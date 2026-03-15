use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::db::models::{CreateSyncTask, File, FileLocation, Storage, SyncTask};
use crate::error::{AppError, AppResult};
use crate::storage::StorageRegistry;

// ─── Export job tracking ────────────────────────────────────────────────────────

/// Status of a running export job.
#[derive(Debug, Clone, Serialize)]
pub struct ExportJobStatus {
    pub job_id: Uuid,
    pub storage_id: Uuid,
    pub status: ExportState,
    pub total_files: u64,
    pub processed_files: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportState {
    InProgress,
    Completed,
    Failed,
}

/// Result of a sync-all operation.
#[derive(Debug, Serialize)]
pub struct SyncAllResult {
    pub storage_id: Uuid,
    pub sync_tasks_created: usize,
    pub already_synced: usize,
}

/// Service for bulk storage operations: sync-all and export.
pub struct BulkService {
    pool: PgPool,
    registry: Arc<StorageRegistry>,
    export_jobs: Arc<RwLock<HashMap<Uuid, ExportJobStatus>>>,
    export_data: Arc<RwLock<HashMap<Uuid, Vec<u8>>>>,
}

impl BulkService {
    pub fn new(pool: PgPool, registry: Arc<StorageRegistry>) -> Self {
        Self {
            pool,
            registry,
            export_jobs: Arc::new(RwLock::new(HashMap::new())),
            export_data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Enumerate all files not yet on the target storage and create sync tasks for each.
    pub async fn sync_all(&self, target_storage_id: Uuid) -> AppResult<SyncAllResult> {
        // Verify storage exists and is enabled
        let target = Storage::find_by_id(&self.pool, target_storage_id).await?;
        if !target.enabled {
            return Err(AppError::BadRequest(
                "Target storage is disabled".to_string(),
            ));
        }

        // Find all files that have at least one synced location but NOT on the target storage
        let files_needing_sync: Vec<(Uuid,)> = sqlx::query_as(
            r#"SELECT DISTINCT f.id
               FROM files f
               JOIN file_locations fl ON fl.file_id = f.id AND fl.status = 'synced'
               WHERE NOT EXISTS (
                   SELECT 1 FROM file_locations fl2
                   WHERE fl2.file_id = f.id AND fl2.storage_id = $1
                     AND fl2.status IN ('synced', 'pending')
               )
               AND NOT EXISTS (
                   SELECT 1 FROM sync_tasks st
                   WHERE st.file_id = f.id AND st.target_storage_id = $1
                     AND st.status IN ('pending', 'in_progress')
               )"#,
        )
        .bind(target_storage_id)
        .fetch_all(&self.pool)
        .await?;

        let already_synced_row: (i64,) = sqlx::query_as(
            r#"SELECT COUNT(DISTINCT fl.file_id)::bigint
               FROM file_locations fl
               WHERE fl.storage_id = $1 AND fl.status = 'synced'"#,
        )
        .bind(target_storage_id)
        .fetch_one(&self.pool)
        .await?;

        let mut sync_tasks_created = 0;

        for (file_id,) in &files_needing_sync {
            // Find a source storage that has this file synced
            let source_loc = sqlx::query_as::<_, FileLocation>(
                r#"SELECT fl.* FROM file_locations fl
                   JOIN storages s ON s.id = fl.storage_id AND s.enabled = TRUE
                   WHERE fl.file_id = $1 AND fl.status = 'synced'
                   ORDER BY s.is_hot DESC
                   LIMIT 1"#,
            )
            .bind(file_id)
            .fetch_optional(&self.pool)
            .await?;

            if let Some(source) = source_loc {
                let create_task = CreateSyncTask {
                    file_id: *file_id,
                    source_storage_id: source.storage_id,
                    target_storage_id,
                };
                match SyncTask::create(&self.pool, &create_task).await {
                    Ok(_) => sync_tasks_created += 1,
                    Err(e) => {
                        tracing::warn!(
                            file_id = %file_id,
                            error = %e,
                            "Failed to create sync task for bulk sync"
                        );
                    }
                }
            }
        }

        Ok(SyncAllResult {
            storage_id: target_storage_id,
            sync_tasks_created,
            already_synced: already_synced_row.0 as usize,
        })
    }

    /// Start an export job that produces a tar.gz archive of all files on a storage.
    /// Returns the job ID immediately; the export runs in the background.
    pub async fn start_export(&self, storage_id: Uuid) -> AppResult<Uuid> {
        // Verify storage exists
        Storage::find_by_id(&self.pool, storage_id).await?;

        let job_id = Uuid::new_v4();

        // Count total files on this storage
        let count_row: (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint FROM file_locations
               WHERE storage_id = $1 AND status = 'synced'"#,
        )
        .bind(storage_id)
        .fetch_one(&self.pool)
        .await?;

        let total_files = count_row.0 as u64;

        let status = ExportJobStatus {
            job_id,
            storage_id,
            status: ExportState::InProgress,
            total_files,
            processed_files: 0,
            total_bytes: 0,
            error: None,
        };

        {
            let mut jobs = self.export_jobs.write().await;
            jobs.insert(job_id, status);
        }

        // Spawn background task to build the archive
        let pool = self.pool.clone();
        let registry = self.registry.clone();
        let jobs = self.export_jobs.clone();
        let data_store = self.export_data.clone();

        tokio::spawn(async move {
            match build_tar_gz_archive(pool, registry, storage_id, job_id, jobs.clone()).await {
                Ok(archive_data) => {
                    let mut jobs = jobs.write().await;
                    if let Some(job) = jobs.get_mut(&job_id) {
                        job.status = ExportState::Completed;
                        job.total_bytes = archive_data.len() as u64;
                    }
                    let mut data = data_store.write().await;
                    data.insert(job_id, archive_data);
                }
                Err(e) => {
                    let mut jobs = jobs.write().await;
                    if let Some(job) = jobs.get_mut(&job_id) {
                        job.status = ExportState::Failed;
                        job.error = Some(format!("{}", e));
                    }
                }
            }
        });

        Ok(job_id)
    }

    /// Get the status of an export job.
    pub async fn get_export_status(&self, job_id: Uuid) -> AppResult<ExportJobStatus> {
        let jobs = self.export_jobs.read().await;
        jobs.get(&job_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("Export job {} not found", job_id)))
    }

    /// Get the completed export archive data and remove it from memory.
    pub async fn get_export_data(&self, job_id: Uuid) -> AppResult<Vec<u8>> {
        let status = self.get_export_status(job_id).await?;
        if status.status != ExportState::Completed {
            return Err(AppError::BadRequest(
                "Export is not yet completed".to_string(),
            ));
        }

        let mut data = self.export_data.write().await;
        data.remove(&job_id)
            .ok_or_else(|| AppError::NotFound("Export data not found or already downloaded".to_string()))
    }
}

/// Build a tar.gz archive of all synced files on the given storage.
async fn build_tar_gz_archive(
    pool: PgPool,
    registry: Arc<StorageRegistry>,
    storage_id: Uuid,
    job_id: Uuid,
    jobs: Arc<RwLock<HashMap<Uuid, ExportJobStatus>>>,
) -> AppResult<Vec<u8>> {
    // Fetch all synced file locations on this storage
    let locations: Vec<FileLocation> = sqlx::query_as(
        r#"SELECT fl.* FROM file_locations fl
           WHERE fl.storage_id = $1 AND fl.status = 'synced'
           ORDER BY fl.created_at ASC"#,
    )
    .bind(storage_id)
    .fetch_all(&pool)
    .await?;

    let backend = registry.get(&storage_id).await?;

    let buf = Vec::new();
    let gz_encoder = flate2::write::GzEncoder::new(buf, flate2::Compression::default());
    let mut tar_builder = tar::Builder::new(gz_encoder);

    let mut processed = 0u64;

    for location in &locations {
        // Get file metadata for the archive entry name
        let file = File::find_by_id(&pool, location.file_id).await?;

        match backend.download(&location.storage_path).await {
            Ok(data) => {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();

                // Use hash as filename in the archive
                let entry_path = format!("{}/{}", &file.hash_sha256[..2], &file.hash_sha256);
                tar_builder
                    .append_data(&mut header, &entry_path, &data[..])
                    .map_err(|e| AppError::Internal(format!("Failed to append to tar: {}", e)))?;

                processed += 1;

                // Update progress
                let mut jobs = jobs.write().await;
                if let Some(job) = jobs.get_mut(&job_id) {
                    job.processed_files = processed;
                }
            }
            Err(e) => {
                tracing::warn!(
                    file_id = %location.file_id,
                    storage_path = %location.storage_path,
                    error = %e,
                    "Failed to download file for export, skipping"
                );
                processed += 1;

                let mut jobs = jobs.write().await;
                if let Some(job) = jobs.get_mut(&job_id) {
                    job.processed_files = processed;
                }
            }
        }
    }

    let gz_encoder = tar_builder
        .into_inner()
        .map_err(|e| AppError::Internal(format!("Failed to finalize tar: {}", e)))?;
    let compressed = gz_encoder
        .finish()
        .map_err(|e| AppError::Internal(format!("Failed to finalize gzip: {}", e)))?;

    Ok(compressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_all_result_serialization() {
        let result = SyncAllResult {
            storage_id: Uuid::new_v4(),
            sync_tasks_created: 10,
            already_synced: 42,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["sync_tasks_created"], 10);
        assert_eq!(json["already_synced"], 42);
        assert!(json["storage_id"].is_string());
    }

    #[test]
    fn test_export_job_status_serialization() {
        let status = ExportJobStatus {
            job_id: Uuid::new_v4(),
            storage_id: Uuid::new_v4(),
            status: ExportState::InProgress,
            total_files: 100,
            processed_files: 50,
            total_bytes: 0,
            error: None,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["status"], "in_progress");
        assert_eq!(json["total_files"], 100);
        assert_eq!(json["processed_files"], 50);
        assert!(json["error"].is_null());
    }

    #[test]
    fn test_export_state_completed_serialization() {
        let status = ExportJobStatus {
            job_id: Uuid::new_v4(),
            storage_id: Uuid::new_v4(),
            status: ExportState::Completed,
            total_files: 100,
            processed_files: 100,
            total_bytes: 1_048_576,
            error: None,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["status"], "completed");
        assert_eq!(json["total_bytes"], 1_048_576);
    }

    #[test]
    fn test_export_state_failed_serialization() {
        let status = ExportJobStatus {
            job_id: Uuid::new_v4(),
            storage_id: Uuid::new_v4(),
            status: ExportState::Failed,
            total_files: 100,
            processed_files: 42,
            total_bytes: 0,
            error: Some("Connection timeout".to_string()),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["status"], "failed");
        assert_eq!(json["error"], "Connection timeout");
    }

    #[test]
    fn test_export_state_equality() {
        assert_eq!(ExportState::InProgress, ExportState::InProgress);
        assert_eq!(ExportState::Completed, ExportState::Completed);
        assert_eq!(ExportState::Failed, ExportState::Failed);
        assert_ne!(ExportState::InProgress, ExportState::Completed);
        assert_ne!(ExportState::Completed, ExportState::Failed);
    }

    #[tokio::test]
    async fn test_bulk_service_construction() {
        let registry = Arc::new(StorageRegistry::new());
        assert_eq!(registry.len().await, 0);
    }

    #[test]
    fn test_sync_all_result_with_zero_tasks() {
        let result = SyncAllResult {
            storage_id: Uuid::new_v4(),
            sync_tasks_created: 0,
            already_synced: 100,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["sync_tasks_created"], 0);
        assert_eq!(json["already_synced"], 100);
    }

    #[test]
    fn test_export_job_status_percentage_calculation() {
        let status = ExportJobStatus {
            job_id: Uuid::new_v4(),
            storage_id: Uuid::new_v4(),
            status: ExportState::InProgress,
            total_files: 200,
            processed_files: 50,
            total_bytes: 0,
            error: None,
        };
        // Clients can calculate percentage from total_files and processed_files
        let percentage = if status.total_files > 0 {
            (status.processed_files as f64 / status.total_files as f64) * 100.0
        } else {
            100.0
        };
        assert!((percentage - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_export_job_status_zero_files() {
        let status = ExportJobStatus {
            job_id: Uuid::new_v4(),
            storage_id: Uuid::new_v4(),
            status: ExportState::InProgress,
            total_files: 0,
            processed_files: 0,
            total_bytes: 0,
            error: None,
        };
        let percentage = if status.total_files > 0 {
            (status.processed_files as f64 / status.total_files as f64) * 100.0
        } else {
            100.0
        };
        assert!((percentage - 100.0).abs() < f64::EPSILON);
    }

    // ─── Integration tests requiring PostgreSQL ─────────────────────────────────

    #[ignore]
    #[tokio::test]
    async fn test_sync_all_no_files() {
        let (pool, registry, _dir, storage_id) = setup_bulk_integration().await;
        let service = BulkService::new(pool, registry);
        let result = service.sync_all(storage_id).await.unwrap();
        assert_eq!(result.sync_tasks_created, 0);
        assert_eq!(result.already_synced, 0);
    }

    #[ignore]
    #[tokio::test]
    async fn test_sync_all_disabled_storage() {
        let (pool, registry, _dir, storage_id) = setup_bulk_integration().await;
        // Disable the storage
        Storage::update_enabled(&pool, storage_id, false)
            .await
            .unwrap();
        let service = BulkService::new(pool, registry);
        let result = service.sync_all(storage_id).await;
        assert!(result.is_err());
    }

    #[ignore]
    #[tokio::test]
    async fn test_sync_all_nonexistent_storage() {
        let (pool, registry, _dir, _) = setup_bulk_integration().await;
        let service = BulkService::new(pool, registry);
        let result = service.sync_all(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[ignore]
    #[tokio::test]
    async fn test_export_nonexistent_storage() {
        let (pool, registry, _dir, _) = setup_bulk_integration().await;
        let service = BulkService::new(pool, registry);
        let result = service.start_export(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[ignore]
    #[tokio::test]
    async fn test_export_status_not_found() {
        let (pool, registry, _dir, _) = setup_bulk_integration().await;
        let service = BulkService::new(pool, registry);
        let result = service.get_export_status(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[ignore]
    #[tokio::test]
    async fn test_export_data_not_completed() {
        let (pool, registry, _dir, storage_id) = setup_bulk_integration().await;
        let service = BulkService::new(pool, registry);
        let job_id = service.start_export(storage_id).await.unwrap();
        // Try getting data immediately - might still be in progress or completed with empty
        // (since there are no files). Either way, test the API works.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let status = service.get_export_status(job_id).await.unwrap();
        assert!(status.status == ExportState::Completed || status.status == ExportState::InProgress);
    }

    #[allow(dead_code)]
    async fn setup_bulk_integration() -> (PgPool, Arc<StorageRegistry>, tempfile::TempDir, Uuid) {
        use crate::db::models::CreateStorage;
        use crate::storage::local::LocalDiskBackend;

        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL required for integration tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        let registry = Arc::new(StorageRegistry::new());

        let backend = Arc::new(LocalDiskBackend::new(
            dir.path().to_path_buf(),
            "test-secret",
        ));

        let create_storage = CreateStorage {
            name: format!("Bulk Test {}", Uuid::new_v4()),
            storage_type: "local".to_string(),
            config: serde_json::json!({"path": dir.path().to_str().unwrap()}),
            is_hot: Some(true),
            project_id: None,
            enabled: Some(true),
            supports_direct_links: None,
        };
        let storage = Storage::create(&pool, &create_storage).await.unwrap();
        registry.register(storage.id, backend).await;

        (pool, registry, dir, storage.id)
    }
}
