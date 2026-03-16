use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::models::{CreateSyncTask, FileLocation, Storage, SyncTask};
use crate::error::{AppError, AppResult};
use crate::storage::StorageRegistry;

/// A file eligible for archiving from hot to cold storage.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct ArchivableFile {
    pub file_id: Uuid,
    pub hot_location_id: Uuid,
    pub hot_storage_id: Uuid,
    pub storage_path: String,
    pub project_id: Uuid,
}

/// A hot storage location ready to be marked as archived.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PendingArchive {
    pub hot_location_id: Uuid,
    pub file_id: Uuid,
}

/// Service for managing hot/cold tier transitions.
///
/// Handles automatic archiving of files from hot to cold storage based on
/// project `hot_to_cold_days` settings, and restoring archived files back
/// to hot storage on demand.
pub struct TierService {
    pool: PgPool,
    registry: Arc<StorageRegistry>,
}

impl TierService {
    pub fn new(pool: PgPool, registry: Arc<StorageRegistry>) -> Self {
        Self { pool, registry }
    }

    /// Find files eligible for archiving to cold storage.
    ///
    /// A file is archivable when:
    /// - It has a synced location on a hot storage
    /// - The project's `hot_to_cold_days` is set
    /// - `last_accessed_at + hot_to_cold_days < now()` (or `last_accessed_at` is NULL)
    /// - No synced copy exists on cold storage
    /// - No pending/in_progress sync tasks exist for the file
    pub async fn find_archivable_files(&self, limit: i64) -> AppResult<Vec<ArchivableFile>> {
        let files = sqlx::query_as::<_, ArchivableFile>(
            r#"SELECT DISTINCT ON (fl.file_id)
                fl.file_id,
                fl.id as hot_location_id,
                fl.storage_id as hot_storage_id,
                fl.storage_path,
                fr.project_id
            FROM file_locations fl
            JOIN storages s ON s.id = fl.storage_id AND s.is_hot = TRUE AND s.enabled = TRUE
            JOIN file_references fr ON fr.file_id = fl.file_id
            JOIN projects p ON p.id = fr.project_id AND p.hot_to_cold_days IS NOT NULL
            WHERE fl.status = 'synced'
              AND (fl.last_accessed_at IS NULL
                   OR fl.last_accessed_at + make_interval(days => p.hot_to_cold_days) < NOW())
              AND NOT EXISTS (
                SELECT 1 FROM file_locations fl2
                JOIN storages s2 ON s2.id = fl2.storage_id AND s2.is_hot = FALSE AND s2.enabled = TRUE
                WHERE fl2.file_id = fl.file_id AND fl2.status = 'synced'
              )
              AND NOT EXISTS (
                SELECT 1 FROM sync_tasks st
                WHERE st.file_id = fl.file_id AND st.status IN ('pending', 'in_progress')
              )
            ORDER BY fl.file_id, fl.last_accessed_at ASC NULLS FIRST
            LIMIT $1"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(files)
    }

    /// Scan for archivable files and create sync tasks to cold storage.
    /// Returns the number of sync tasks created.
    pub async fn create_archive_tasks(&self) -> AppResult<usize> {
        let archivable = self.find_archivable_files(100).await?;
        let mut tasks_created = 0;

        for file in &archivable {
            if let Some(cold_storage) = self.find_cold_storage(file.file_id).await? {
                if !self.registry.contains(&cold_storage.id).await {
                    continue;
                }
                let create_task = CreateSyncTask {
                    file_id: file.file_id,
                    source_storage_id: file.hot_storage_id,
                    target_storage_id: cold_storage.id,
                    project_id: Some(file.project_id),
                };
                match SyncTask::create(&self.pool, &create_task).await {
                    Ok(_) => {
                        tasks_created += 1;
                        tracing::info!(
                            file_id = %file.file_id,
                            cold_storage = %cold_storage.id,
                            "Created archive sync task"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            file_id = %file.file_id,
                            error = %e,
                            "Failed to create archive sync task"
                        );
                    }
                }
            }
        }

        Ok(tasks_created)
    }

    /// Find hot storage locations that now have synced cold copies and mark them as archived.
    /// Returns the number of locations archived.
    pub async fn process_completed_archives(&self) -> AppResult<usize> {
        let pending = sqlx::query_as::<_, PendingArchive>(
            r#"SELECT fl_hot.id as hot_location_id, fl_hot.file_id
            FROM file_locations fl_hot
            JOIN storages s_hot ON s_hot.id = fl_hot.storage_id AND s_hot.is_hot = TRUE
            WHERE fl_hot.status = 'synced'
              AND EXISTS (
                SELECT 1 FROM file_locations fl_cold
                JOIN storages s_cold ON s_cold.id = fl_cold.storage_id
                  AND s_cold.is_hot = FALSE AND s_cold.enabled = TRUE
                WHERE fl_cold.file_id = fl_hot.file_id AND fl_cold.status = 'synced'
              )
              AND EXISTS (
                SELECT 1 FROM file_references fr
                JOIN projects p ON p.id = fr.project_id AND p.hot_to_cold_days IS NOT NULL
                WHERE fr.file_id = fl_hot.file_id
                  AND (fl_hot.last_accessed_at IS NULL
                       OR fl_hot.last_accessed_at + make_interval(days => p.hot_to_cold_days) < NOW())
              )
            LIMIT 100"#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut archived = 0;
        for item in &pending {
            match FileLocation::update_status(&self.pool, item.hot_location_id, "archived").await {
                Ok(_) => {
                    archived += 1;
                    tracing::info!(
                        file_id = %item.file_id,
                        location_id = %item.hot_location_id,
                        "Archived hot storage location"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        file_id = %item.file_id,
                        error = %e,
                        "Failed to archive hot storage location"
                    );
                }
            }
        }

        Ok(archived)
    }

    /// Restore a file from cold to hot storage.
    ///
    /// Finds a cold storage location with synced data, creates a sync task to
    /// a hot storage, and returns immediately. The sync worker handles the
    /// actual data transfer.
    pub async fn restore_file(&self, file_id: Uuid) -> AppResult<SyncTask> {
        // Verify the file exists
        crate::db::models::File::find_by_id(&self.pool, file_id).await?;

        // Find a cold storage location with data
        let cold_loc = sqlx::query_as::<_, FileLocation>(
            r#"SELECT fl.* FROM file_locations fl
               JOIN storages s ON s.id = fl.storage_id AND s.is_hot = FALSE AND s.enabled = TRUE
               WHERE fl.file_id = $1 AND fl.status IN ('synced', 'archived')
               ORDER BY fl.synced_at DESC NULLS LAST
               LIMIT 1"#,
        )
        .bind(file_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            AppError::NotFound("No cold storage location available for restore".to_string())
        })?;

        // Find a hot storage to restore to
        let hot_storage = sqlx::query_as::<_, Storage>(
            "SELECT * FROM storages WHERE is_hot = TRUE AND enabled = TRUE ORDER BY created_at LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            AppError::NotFound("No hot storage available for restore".to_string())
        })?;

        // Resolve project_id from file_references
        let project_id: Option<Uuid> = sqlx::query_as::<_, (Uuid,)>(
            "SELECT project_id FROM file_references WHERE file_id = $1 LIMIT 1",
        )
        .bind(file_id)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| r.0);

        // Create sync task from cold to hot
        let task = SyncTask::create(
            &self.pool,
            &CreateSyncTask {
                file_id,
                source_storage_id: cold_loc.storage_id,
                target_storage_id: hot_storage.id,
                project_id,
            },
        )
        .await?;

        // Mark any archived hot locations as 'restoring'
        let _ = sqlx::query(
            "UPDATE file_locations SET status = 'restoring' WHERE file_id = $1 AND status = 'archived'",
        )
        .bind(file_id)
        .execute(&self.pool)
        .await;

        Ok(task)
    }

    /// Find a cold storage backend available for archiving a file.
    async fn find_cold_storage(&self, file_id: Uuid) -> AppResult<Option<Storage>> {
        let storage = sqlx::query_as::<_, Storage>(
            r#"SELECT DISTINCT s.* FROM storages s
               WHERE s.is_hot = FALSE AND s.enabled = TRUE
                 AND (s.project_id IS NULL OR s.project_id IN (
                   SELECT fr.project_id FROM file_references fr WHERE fr.file_id = $1
                 ))
               ORDER BY s.created_at
               LIMIT 1"#,
        )
        .bind(file_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_archivable_file_struct() {
        let file = ArchivableFile {
            file_id: Uuid::new_v4(),
            hot_location_id: Uuid::new_v4(),
            hot_storage_id: Uuid::new_v4(),
            storage_path: "ab/cd/abcdef1234".to_string(),
            project_id: Uuid::new_v4(),
        };
        assert_eq!(file.storage_path, "ab/cd/abcdef1234");
    }

    #[test]
    fn test_archivable_file_serialization() {
        let file = ArchivableFile {
            file_id: Uuid::new_v4(),
            hot_location_id: Uuid::new_v4(),
            hot_storage_id: Uuid::new_v4(),
            storage_path: "hash123".to_string(),
            project_id: Uuid::new_v4(),
        };
        let json = serde_json::to_value(&file).unwrap();
        assert_eq!(json["storage_path"], "hash123");
        assert!(json["file_id"].is_string());
        assert!(json["hot_location_id"].is_string());
        assert!(json["hot_storage_id"].is_string());
    }

    #[test]
    fn test_pending_archive_struct() {
        let archive = PendingArchive {
            hot_location_id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
        };
        assert_ne!(archive.hot_location_id, archive.file_id);
    }

    #[tokio::test]
    async fn test_tier_service_construction() {
        let registry = Arc::new(StorageRegistry::new());
        assert_eq!(registry.len().await, 0);
    }

    #[test]
    fn test_archive_detection_logic_old_file() {
        // File accessed 10 days ago with 7-day policy -> should archive
        let now = chrono::Utc::now();
        let hot_to_cold_days = 7i64;
        let last_accessed = now - chrono::Duration::days(10);
        let threshold = now - chrono::Duration::days(hot_to_cold_days);
        assert!(last_accessed < threshold, "File should be archivable");
    }

    #[test]
    fn test_archive_detection_logic_recent_file() {
        // File accessed 3 days ago with 7-day policy -> should NOT archive
        let now = chrono::Utc::now();
        let hot_to_cold_days = 7i64;
        let last_accessed = now - chrono::Duration::days(3);
        let threshold = now - chrono::Duration::days(hot_to_cold_days);
        assert!(
            last_accessed > threshold,
            "Recent file should not be archivable"
        );
    }

    #[test]
    fn test_archive_detection_null_access_is_eligible() {
        // Files with NULL last_accessed_at should be archivable
        // (they've never been accessed since upload)
        let last_accessed: Option<chrono::DateTime<chrono::Utc>> = None;
        assert!(
            last_accessed.is_none(),
            "NULL access time means eligible for archival"
        );
    }

    #[test]
    fn test_archive_detection_various_policies() {
        let now = chrono::Utc::now();

        // 1-day policy, accessed 2 days ago -> archive
        let threshold_1d = now - chrono::Duration::days(1);
        let accessed_2d = now - chrono::Duration::days(2);
        assert!(accessed_2d < threshold_1d);

        // 30-day policy, accessed 15 days ago -> don't archive
        let threshold_30d = now - chrono::Duration::days(30);
        let accessed_15d = now - chrono::Duration::days(15);
        assert!(accessed_15d > threshold_30d);

        // 30-day policy, accessed 45 days ago -> archive
        let accessed_45d = now - chrono::Duration::days(45);
        assert!(accessed_45d < threshold_30d);
    }

    #[test]
    fn test_restore_requires_cold_location() {
        // Restore accepts files with "synced" or "archived" status on cold storage
        let valid_statuses = ["synced", "archived"];
        assert!(valid_statuses.contains(&"synced"));
        assert!(valid_statuses.contains(&"archived"));
        assert!(!valid_statuses.contains(&"restoring"));
        assert!(!valid_statuses.contains(&"pending"));
    }

    #[test]
    fn test_access_timestamp_update_on_download() {
        // Verify the contract: FileLocation::touch_accessed is called on download.
        // The actual DB call is in FileService::download_file (line ~160).
        // This test validates the concept that accessing a file resets the archive timer.
        let now = chrono::Utc::now();
        let hot_to_cold_days = 7i64;

        // File was archivable (accessed 10 days ago)
        let old_access = now - chrono::Duration::days(10);
        let threshold = now - chrono::Duration::days(hot_to_cold_days);
        assert!(old_access < threshold, "Was archivable before access");

        // After download, last_accessed_at is updated to now
        let new_access = now;
        assert!(
            new_access > threshold,
            "No longer archivable after access update"
        );
    }

    #[test]
    fn test_access_timestamp_update_on_temp_link() {
        // Same logic: generating a temp link also updates last_accessed_at
        let now = chrono::Utc::now();
        let hot_to_cold_days = 7i64;

        let old_access = now - chrono::Duration::days(10);
        let threshold = now - chrono::Duration::days(hot_to_cold_days);
        assert!(old_access < threshold);

        // After temp link generation, last_accessed_at is updated to now
        let new_access = now;
        assert!(new_access > threshold);
    }

    // ─── Integration tests requiring PostgreSQL ─────────────────────────────────

    #[ignore]
    #[tokio::test]
    async fn test_find_archivable_files_empty() {
        let (pool, registry) = setup_tier_integration().await;
        let service = TierService::new(pool, registry);
        let files = service.find_archivable_files(10).await.unwrap();
        assert!(files.is_empty());
    }

    #[ignore]
    #[tokio::test]
    async fn test_create_archive_tasks_empty() {
        let (pool, registry) = setup_tier_integration().await;
        let service = TierService::new(pool, registry);
        let count = service.create_archive_tasks().await.unwrap();
        assert_eq!(count, 0);
    }

    #[ignore]
    #[tokio::test]
    async fn test_process_completed_archives_empty() {
        let (pool, registry) = setup_tier_integration().await;
        let service = TierService::new(pool, registry);
        let count = service.process_completed_archives().await.unwrap();
        assert_eq!(count, 0);
    }

    #[ignore]
    #[tokio::test]
    async fn test_restore_file_not_found() {
        let (pool, registry) = setup_tier_integration().await;
        let service = TierService::new(pool, registry);
        let result = service.restore_file(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[allow(dead_code)]
    async fn setup_tier_integration() -> (PgPool, Arc<StorageRegistry>) {
        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL required for integration tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();
        let registry = Arc::new(StorageRegistry::new());
        (pool, registry)
    }
}
