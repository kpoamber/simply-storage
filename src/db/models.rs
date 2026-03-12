use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

// ─── Project ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub hot_to_cold_days: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProject {
    pub name: String,
    pub slug: String,
    pub hot_to_cold_days: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub hot_to_cold_days: Option<Option<i32>>,
}

impl Project {
    pub async fn create(pool: &PgPool, input: &CreateProject) -> AppResult<Project> {
        let row = sqlx::query_as::<_, Project>(
            r#"INSERT INTO projects (name, slug, hot_to_cold_days)
               VALUES ($1, $2, $3)
               RETURNING *"#,
        )
        .bind(&input.name)
        .bind(&input.slug)
        .bind(input.hot_to_cold_days)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> AppResult<Project> {
        sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Project {} not found", id)))
    }

    pub async fn find_by_slug(pool: &PgPool, slug: &str) -> AppResult<Project> {
        sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE slug = $1")
            .bind(slug)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Project with slug '{}' not found", slug)))
    }

    pub async fn list(pool: &PgPool) -> AppResult<Vec<Project>> {
        let rows = sqlx::query_as::<_, Project>(
            "SELECT * FROM projects ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn update(pool: &PgPool, id: Uuid, input: &UpdateProject) -> AppResult<Project> {
        let current = Self::find_by_id(pool, id).await?;

        let name = input.name.as_deref().unwrap_or(&current.name);
        let slug = input.slug.as_deref().unwrap_or(&current.slug);
        let hot_to_cold_days = match &input.hot_to_cold_days {
            Some(val) => *val,
            None => current.hot_to_cold_days,
        };

        let row = sqlx::query_as::<_, Project>(
            r#"UPDATE projects SET name = $1, slug = $2, hot_to_cold_days = $3
               WHERE id = $4
               RETURNING *"#,
        )
        .bind(name)
        .bind(slug)
        .bind(hot_to_cold_days)
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Project {} not found", id)));
        }
        Ok(())
    }
}

// ─── Storage ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Storage {
    pub id: Uuid,
    pub name: String,
    pub storage_type: String,
    pub config: serde_json::Value,
    pub is_hot: bool,
    pub project_id: Option<Uuid>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateStorage {
    pub name: String,
    pub storage_type: String,
    pub config: serde_json::Value,
    pub is_hot: Option<bool>,
    pub project_id: Option<Uuid>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateStorage {
    pub name: Option<String>,
    pub storage_type: Option<String>,
    pub config: Option<serde_json::Value>,
    pub is_hot: Option<bool>,
    pub project_id: Option<Option<Uuid>>,
    pub enabled: Option<bool>,
}

impl Storage {
    pub async fn create(pool: &PgPool, input: &CreateStorage) -> AppResult<Storage> {
        let row = sqlx::query_as::<_, Storage>(
            r#"INSERT INTO storages (name, storage_type, config, is_hot, project_id, enabled)
               VALUES ($1, $2, $3, $4, $5, $6)
               RETURNING *"#,
        )
        .bind(&input.name)
        .bind(&input.storage_type)
        .bind(&input.config)
        .bind(input.is_hot.unwrap_or(true))
        .bind(input.project_id)
        .bind(input.enabled.unwrap_or(true))
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> AppResult<Storage> {
        sqlx::query_as::<_, Storage>("SELECT * FROM storages WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Storage {} not found", id)))
    }

    pub async fn list(pool: &PgPool) -> AppResult<Vec<Storage>> {
        let rows = sqlx::query_as::<_, Storage>(
            "SELECT * FROM storages ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn list_enabled(pool: &PgPool) -> AppResult<Vec<Storage>> {
        let rows = sqlx::query_as::<_, Storage>(
            "SELECT * FROM storages WHERE enabled = TRUE ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn list_for_project(pool: &PgPool, project_id: Uuid) -> AppResult<Vec<Storage>> {
        let rows = sqlx::query_as::<_, Storage>(
            r#"SELECT * FROM storages
               WHERE enabled = TRUE AND (project_id IS NULL OR project_id = $1)
               ORDER BY created_at DESC"#,
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn update_enabled(pool: &PgPool, id: Uuid, enabled: bool) -> AppResult<Storage> {
        let row = sqlx::query_as::<_, Storage>(
            "UPDATE storages SET enabled = $1 WHERE id = $2 RETURNING *",
        )
        .bind(enabled)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Storage {} not found", id)))?;

        Ok(row)
    }

    pub async fn update(pool: &PgPool, id: Uuid, input: &UpdateStorage) -> AppResult<Storage> {
        let current = Self::find_by_id(pool, id).await?;

        let name = input.name.as_deref().unwrap_or(&current.name);
        let storage_type = input.storage_type.as_deref().unwrap_or(&current.storage_type);
        let config = input.config.as_ref().unwrap_or(&current.config);
        let is_hot = input.is_hot.unwrap_or(current.is_hot);
        let project_id = match &input.project_id {
            Some(val) => *val,
            None => current.project_id,
        };
        let enabled = input.enabled.unwrap_or(current.enabled);

        let row = sqlx::query_as::<_, Storage>(
            r#"UPDATE storages
               SET name = $1, storage_type = $2, config = $3, is_hot = $4, project_id = $5, enabled = $6
               WHERE id = $7
               RETURNING *"#,
        )
        .bind(name)
        .bind(storage_type)
        .bind(config)
        .bind(is_hot)
        .bind(project_id)
        .bind(enabled)
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }
}

// ─── File ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct File {
    pub id: Uuid,
    pub hash_sha256: String,
    pub size: i64,
    pub content_type: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFile {
    pub hash_sha256: String,
    pub size: i64,
    pub content_type: String,
}

impl File {
    pub async fn create(pool: &PgPool, input: &CreateFile) -> AppResult<File> {
        let row = sqlx::query_as::<_, File>(
            r#"INSERT INTO files (hash_sha256, size, content_type)
               VALUES ($1, $2, $3)
               RETURNING *"#,
        )
        .bind(&input.hash_sha256)
        .bind(input.size)
        .bind(&input.content_type)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> AppResult<File> {
        sqlx::query_as::<_, File>("SELECT * FROM files WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("File {} not found", id)))
    }

    pub async fn find_by_hash(pool: &PgPool, hash: &str) -> AppResult<Option<File>> {
        let row = sqlx::query_as::<_, File>(
            "SELECT * FROM files WHERE hash_sha256 = $1",
        )
        .bind(hash)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    pub async fn create_or_find(pool: &PgPool, input: &CreateFile) -> AppResult<(File, bool)> {
        if let Some(existing) = Self::find_by_hash(pool, &input.hash_sha256).await? {
            return Ok((existing, false));
        }

        match Self::create(pool, input).await {
            Ok(file) => Ok((file, true)),
            Err(AppError::Database(ref e)) if is_unique_violation(e) => {
                let file = Self::find_by_hash(pool, &input.hash_sha256)
                    .await?
                    .ok_or_else(|| {
                        AppError::Internal("File disappeared after conflict".to_string())
                    })?;
                Ok((file, false))
            }
            Err(e) => Err(e),
        }
    }
}

// ─── FileReference ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FileReference {
    pub id: Uuid,
    pub file_id: Uuid,
    pub project_id: Uuid,
    pub original_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFileReference {
    pub file_id: Uuid,
    pub project_id: Uuid,
    pub original_name: String,
}

impl FileReference {
    pub async fn create(pool: &PgPool, input: &CreateFileReference) -> AppResult<FileReference> {
        let row = sqlx::query_as::<_, FileReference>(
            r#"INSERT INTO file_references (file_id, project_id, original_name)
               VALUES ($1, $2, $3)
               RETURNING *"#,
        )
        .bind(input.file_id)
        .bind(input.project_id)
        .bind(&input.original_name)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn create_or_find(
        pool: &PgPool,
        input: &CreateFileReference,
    ) -> AppResult<FileReference> {
        match Self::create(pool, input).await {
            Ok(row) => Ok(row),
            Err(AppError::Database(ref e)) if is_unique_violation(e) => {
                let row = sqlx::query_as::<_, FileReference>(
                    r#"SELECT * FROM file_references
                       WHERE file_id = $1 AND project_id = $2 AND original_name = $3"#,
                )
                .bind(input.file_id)
                .bind(input.project_id)
                .bind(&input.original_name)
                .fetch_one(pool)
                .await?;
                Ok(row)
            }
            Err(e) => Err(e),
        }
    }

    pub async fn list_for_project(
        pool: &PgPool,
        project_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> AppResult<Vec<FileReference>> {
        let rows = sqlx::query_as::<_, FileReference>(
            r#"SELECT * FROM file_references
               WHERE project_id = $1
               ORDER BY created_at DESC
               LIMIT $2 OFFSET $3"#,
        )
        .bind(project_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM file_references WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "File reference {} not found",
                id
            )));
        }
        Ok(())
    }

    pub async fn delete_by_file_and_project(
        pool: &PgPool,
        file_id: Uuid,
        project_id: Uuid,
    ) -> AppResult<()> {
        let result = sqlx::query(
            "DELETE FROM file_references WHERE file_id = $1 AND project_id = $2",
        )
        .bind(file_id)
        .bind(project_id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                "File reference not found for this file and project".to_string(),
            ));
        }
        Ok(())
    }
}

// ─── FileLocation ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FileLocation {
    pub id: Uuid,
    pub file_id: Uuid,
    pub storage_id: Uuid,
    pub storage_path: String,
    pub status: String,
    pub synced_at: Option<DateTime<Utc>>,
    pub last_accessed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFileLocation {
    pub file_id: Uuid,
    pub storage_id: Uuid,
    pub storage_path: String,
    pub status: String,
}

impl FileLocation {
    pub async fn create(pool: &PgPool, input: &CreateFileLocation) -> AppResult<FileLocation> {
        let row = sqlx::query_as::<_, FileLocation>(
            r#"INSERT INTO file_locations (file_id, storage_id, storage_path, status)
               VALUES ($1, $2, $3, $4)
               RETURNING *"#,
        )
        .bind(input.file_id)
        .bind(input.storage_id)
        .bind(&input.storage_path)
        .bind(&input.status)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn find_for_file(pool: &PgPool, file_id: Uuid) -> AppResult<Vec<FileLocation>> {
        let rows = sqlx::query_as::<_, FileLocation>(
            r#"SELECT fl.* FROM file_locations fl
               JOIN storages s ON s.id = fl.storage_id
               WHERE fl.file_id = $1 AND s.enabled = TRUE
               ORDER BY s.is_hot DESC, fl.last_accessed_at DESC NULLS LAST"#,
        )
        .bind(file_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        status: &str,
    ) -> AppResult<FileLocation> {
        let synced_clause = if status == "synced" {
            ", synced_at = NOW()"
        } else {
            ""
        };

        let sql = format!(
            "UPDATE file_locations SET status = $1{} WHERE id = $2 RETURNING *",
            synced_clause
        );

        let row = sqlx::query_as::<_, FileLocation>(&sql)
            .bind(status)
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("File location {} not found", id)))?;

        Ok(row)
    }

    pub async fn touch_accessed(pool: &PgPool, id: Uuid) -> AppResult<()> {
        sqlx::query("UPDATE file_locations SET last_accessed_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

// ─── SyncTask ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SyncTask {
    pub id: Uuid,
    pub file_id: Uuid,
    pub source_storage_id: Uuid,
    pub target_storage_id: Uuid,
    pub status: String,
    pub retries: i32,
    pub error_msg: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSyncTask {
    pub file_id: Uuid,
    pub source_storage_id: Uuid,
    pub target_storage_id: Uuid,
}

impl SyncTask {
    pub async fn create(pool: &PgPool, input: &CreateSyncTask) -> AppResult<SyncTask> {
        let row = sqlx::query_as::<_, SyncTask>(
            r#"INSERT INTO sync_tasks (file_id, source_storage_id, target_storage_id)
               VALUES ($1, $2, $3)
               RETURNING *"#,
        )
        .bind(input.file_id)
        .bind(input.source_storage_id)
        .bind(input.target_storage_id)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn find_pending(pool: &PgPool, limit: i64) -> AppResult<Vec<SyncTask>> {
        let rows = sqlx::query_as::<_, SyncTask>(
            r#"SELECT * FROM sync_tasks
               WHERE status = 'pending'
               ORDER BY created_at ASC
               LIMIT $1"#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        status: &str,
        error_msg: Option<&str>,
    ) -> AppResult<SyncTask> {
        let row = sqlx::query_as::<_, SyncTask>(
            r#"UPDATE sync_tasks
               SET status = $1, error_msg = $2, retries = CASE WHEN $1 = 'failed' THEN retries + 1 ELSE retries END
               WHERE id = $3
               RETURNING *"#,
        )
        .bind(status)
        .bind(error_msg)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Sync task {} not found", id)))?;

        Ok(row)
    }

    /// Claim a batch of pending sync tasks using PostgreSQL advisory locks.
    /// Only returns tasks that this worker successfully locked.
    /// Tasks with retries >= max_retries are skipped and marked as 'failed'.
    pub async fn claim_pending(
        pool: &PgPool,
        limit: i64,
        max_retries: i32,
    ) -> AppResult<Vec<SyncTask>> {
        // Mark permanently failed tasks first
        sqlx::query(
            r#"UPDATE sync_tasks SET status = 'failed', error_msg = 'Max retries exceeded'
               WHERE status = 'pending' AND retries >= $1"#,
        )
        .bind(max_retries)
        .execute(pool)
        .await?;

        // Fetch pending tasks and try to lock each one with advisory lock
        // We use the UUID's first 8 bytes as the lock key
        let tasks = sqlx::query_as::<_, SyncTask>(
            r#"SELECT * FROM sync_tasks
               WHERE status = 'pending' AND retries < $1
               ORDER BY created_at ASC
               LIMIT $2"#,
        )
        .bind(max_retries)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let mut claimed = Vec::new();
        for task in tasks {
            // Use task UUID bytes as advisory lock key (two i32s from first 8 bytes)
            let id_bytes = task.id.as_bytes();
            let key1 = i32::from_le_bytes([id_bytes[0], id_bytes[1], id_bytes[2], id_bytes[3]]);
            let key2 = i32::from_le_bytes([id_bytes[4], id_bytes[5], id_bytes[6], id_bytes[7]]);

            let locked: (bool,) = sqlx::query_as(
                "SELECT pg_try_advisory_lock($1, $2)",
            )
            .bind(key1)
            .bind(key2)
            .fetch_one(pool)
            .await?;

            if locked.0 {
                // Mark as in_progress
                sqlx::query(
                    "UPDATE sync_tasks SET status = 'in_progress' WHERE id = $1",
                )
                .bind(task.id)
                .execute(pool)
                .await?;

                claimed.push(SyncTask {
                    status: "in_progress".to_string(),
                    ..task
                });
            }
        }

        Ok(claimed)
    }

    /// Release the advisory lock for a sync task.
    pub async fn release_lock(pool: &PgPool, task_id: Uuid) -> AppResult<()> {
        let id_bytes = task_id.as_bytes();
        let key1 = i32::from_le_bytes([id_bytes[0], id_bytes[1], id_bytes[2], id_bytes[3]]);
        let key2 = i32::from_le_bytes([id_bytes[4], id_bytes[5], id_bytes[6], id_bytes[7]]);

        sqlx::query("SELECT pg_advisory_unlock($1, $2)")
            .bind(key1)
            .bind(key2)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Re-queue a failed task back to pending for retry.
    pub async fn requeue_for_retry(
        pool: &PgPool,
        id: Uuid,
        error_msg: &str,
    ) -> AppResult<SyncTask> {
        let row = sqlx::query_as::<_, SyncTask>(
            r#"UPDATE sync_tasks
               SET status = 'pending', error_msg = $1, retries = retries + 1
               WHERE id = $2
               RETURNING *"#,
        )
        .bind(error_msg)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Sync task {} not found", id)))?;

        Ok(row)
    }

    pub async fn list_filtered(
        pool: &PgPool,
        status: Option<&str>,
        storage_id: Option<Uuid>,
    ) -> AppResult<Vec<SyncTask>> {
        let mut sql = String::from("SELECT * FROM sync_tasks WHERE 1=1");
        let mut param_idx = 1u32;

        if status.is_some() {
            sql.push_str(&format!(" AND status = ${}", param_idx));
            param_idx += 1;
        }
        if storage_id.is_some() {
            sql.push_str(&format!(
                " AND (source_storage_id = ${p} OR target_storage_id = ${p})",
                p = param_idx
            ));
        }
        sql.push_str(" ORDER BY created_at DESC");

        let mut query = sqlx::query_as::<_, SyncTask>(&sql);
        if let Some(s) = status {
            query = query.bind(s);
        }
        if let Some(sid) = storage_id {
            query = query.bind(sid);
        }

        let rows = query.fetch_all(pool).await?;
        Ok(rows)
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────────

fn is_unique_violation(e: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = e {
        // PostgreSQL unique_violation error code
        return db_err.code().as_deref() == Some("23505");
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_project_struct() {
        let input = CreateProject {
            name: "Test Project".to_string(),
            slug: "test-project".to_string(),
            hot_to_cold_days: Some(30),
        };
        assert_eq!(input.name, "Test Project");
        assert_eq!(input.slug, "test-project");
        assert_eq!(input.hot_to_cold_days, Some(30));
    }

    #[test]
    fn test_create_storage_struct() {
        let input = CreateStorage {
            name: "S3 Primary".to_string(),
            storage_type: "s3".to_string(),
            config: serde_json::json!({"bucket": "my-bucket", "region": "us-east-1"}),
            is_hot: Some(true),
            project_id: None,
            enabled: None,
        };
        assert_eq!(input.storage_type, "s3");
        assert_eq!(input.config["bucket"], "my-bucket");
    }

    #[test]
    fn test_create_file_struct() {
        let input = CreateFile {
            hash_sha256: "a".repeat(64),
            size: 1024,
            content_type: "text/plain".to_string(),
        };
        assert_eq!(input.hash_sha256.len(), 64);
        assert_eq!(input.size, 1024);
    }

    #[test]
    fn test_project_serialization() {
        let now = Utc::now();
        let project = Project {
            id: Uuid::new_v4(),
            name: "My Project".to_string(),
            slug: "my-project".to_string(),
            hot_to_cold_days: Some(7),
            created_at: now,
            updated_at: now,
        };

        let json = serde_json::to_value(&project).unwrap();
        assert_eq!(json["name"], "My Project");
        assert_eq!(json["slug"], "my-project");
        assert_eq!(json["hot_to_cold_days"], 7);
    }

    #[test]
    fn test_storage_serialization() {
        let now = Utc::now();
        let storage = Storage {
            id: Uuid::new_v4(),
            name: "Local Disk".to_string(),
            storage_type: "local".to_string(),
            config: serde_json::json!({"path": "/data"}),
            is_hot: true,
            project_id: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        };

        let json = serde_json::to_value(&storage).unwrap();
        assert_eq!(json["storage_type"], "local");
        assert!(json["is_hot"].as_bool().unwrap());
        assert!(json["project_id"].is_null());
    }

    #[test]
    fn test_file_serialization() {
        let now = Utc::now();
        let file = File {
            id: Uuid::new_v4(),
            hash_sha256: "abcd".repeat(16),
            size: 2048,
            content_type: "image/png".to_string(),
            created_at: now,
        };

        let json = serde_json::to_value(&file).unwrap();
        assert_eq!(json["size"], 2048);
        assert_eq!(json["content_type"], "image/png");
    }

    #[test]
    fn test_file_reference_serialization() {
        let now = Utc::now();
        let fref = FileReference {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            original_name: "photo.jpg".to_string(),
            created_at: now,
        };

        let json = serde_json::to_value(&fref).unwrap();
        assert_eq!(json["original_name"], "photo.jpg");
    }

    #[test]
    fn test_file_location_serialization() {
        let now = Utc::now();
        let loc = FileLocation {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            storage_id: Uuid::new_v4(),
            storage_path: "ab/cd/abcdef1234".to_string(),
            status: "synced".to_string(),
            synced_at: Some(now),
            last_accessed_at: Some(now),
            created_at: now,
        };

        let json = serde_json::to_value(&loc).unwrap();
        assert_eq!(json["status"], "synced");
        assert_eq!(json["storage_path"], "ab/cd/abcdef1234");
        assert!(!json["synced_at"].is_null());
    }

    #[test]
    fn test_sync_task_serialization() {
        let now = Utc::now();
        let task = SyncTask {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            source_storage_id: Uuid::new_v4(),
            target_storage_id: Uuid::new_v4(),
            status: "pending".to_string(),
            retries: 0,
            error_msg: None,
            created_at: now,
            updated_at: now,
        };

        let json = serde_json::to_value(&task).unwrap();
        assert_eq!(json["status"], "pending");
        assert_eq!(json["retries"], 0);
        assert!(json["error_msg"].is_null());
    }

    #[test]
    fn test_sync_task_with_error() {
        let now = Utc::now();
        let task = SyncTask {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            source_storage_id: Uuid::new_v4(),
            target_storage_id: Uuid::new_v4(),
            status: "failed".to_string(),
            retries: 3,
            error_msg: Some("Connection timeout".to_string()),
            created_at: now,
            updated_at: now,
        };

        let json = serde_json::to_value(&task).unwrap();
        assert_eq!(json["status"], "failed");
        assert_eq!(json["retries"], 3);
        assert_eq!(json["error_msg"], "Connection timeout");
    }

    #[test]
    fn test_update_project_partial() {
        let input = UpdateProject {
            name: Some("New Name".to_string()),
            slug: None,
            hot_to_cold_days: None,
        };
        assert_eq!(input.name, Some("New Name".to_string()));
        assert!(input.slug.is_none());
        assert!(input.hot_to_cold_days.is_none());
    }

    #[test]
    fn test_update_project_clear_hot_to_cold() {
        let input = UpdateProject {
            name: None,
            slug: None,
            hot_to_cold_days: Some(None),
        };
        assert_eq!(input.hot_to_cold_days, Some(None));
    }

    #[test]
    fn test_is_unique_violation() {
        // This tests the helper with a non-DB error
        let err = sqlx::Error::RowNotFound;
        assert!(!is_unique_violation(&err));
    }

    #[test]
    fn test_project_deserialization() {
        let json = serde_json::json!({
            "name": "Test",
            "slug": "test",
            "hot_to_cold_days": null
        });
        let input: CreateProject = serde_json::from_value(json).unwrap();
        assert_eq!(input.name, "Test");
        assert!(input.hot_to_cold_days.is_none());
    }

    #[test]
    fn test_storage_deserialization_defaults() {
        let json = serde_json::json!({
            "name": "My S3",
            "storage_type": "s3",
            "config": {"bucket": "test"}
        });
        let input: CreateStorage = serde_json::from_value(json).unwrap();
        assert!(input.is_hot.is_none());
        assert!(input.project_id.is_none());
        assert!(input.enabled.is_none());
    }

    // ─── Integration tests that require a running PostgreSQL ───────────────────
    // Run with: DATABASE_URL=postgres://... cargo test -- --ignored

    #[ignore]
    #[tokio::test]
    async fn test_migration_applies() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Verify tables exist by querying information_schema
        let tables: Vec<(String,)> = sqlx::query_as(
            r#"SELECT table_name::text FROM information_schema.tables
               WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
               ORDER BY table_name"#,
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let table_names: Vec<&str> = tables.iter().map(|t| t.0.as_str()).collect();
        assert!(table_names.contains(&"projects"));
        assert!(table_names.contains(&"storages"));
        assert!(table_names.contains(&"files"));
        assert!(table_names.contains(&"file_references"));
        assert!(table_names.contains(&"file_locations"));
        assert!(table_names.contains(&"sync_tasks"));
    }

    #[ignore]
    #[tokio::test]
    async fn test_project_crud() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Create
        let input = CreateProject {
            name: "CRUD Test".to_string(),
            slug: format!("crud-test-{}", Uuid::new_v4()),
            hot_to_cold_days: Some(14),
        };
        let project = Project::create(&pool, &input).await.unwrap();
        assert_eq!(project.name, "CRUD Test");
        assert_eq!(project.hot_to_cold_days, Some(14));

        // Read
        let found = Project::find_by_id(&pool, project.id).await.unwrap();
        assert_eq!(found.id, project.id);

        // Update
        let update = UpdateProject {
            name: Some("Updated Name".to_string()),
            slug: None,
            hot_to_cold_days: None,
        };
        let updated = Project::update(&pool, project.id, &update).await.unwrap();
        assert_eq!(updated.name, "Updated Name");
        assert_eq!(updated.slug, project.slug);

        // List
        let all = Project::list(&pool).await.unwrap();
        assert!(all.iter().any(|p| p.id == project.id));

        // Delete
        Project::delete(&pool, project.id).await.unwrap();
        let result = Project::find_by_id(&pool, project.id).await;
        assert!(result.is_err());
    }

    #[ignore]
    #[tokio::test]
    async fn test_unique_constraints() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Project slug uniqueness
        let slug = format!("unique-test-{}", Uuid::new_v4());
        let input = CreateProject {
            name: "First".to_string(),
            slug: slug.clone(),
            hot_to_cold_days: None,
        };
        Project::create(&pool, &input).await.unwrap();

        let input2 = CreateProject {
            name: "Second".to_string(),
            slug,
            hot_to_cold_days: None,
        };
        let result = Project::create(&pool, &input2).await;
        assert!(result.is_err());

        // File hash uniqueness
        let hash = "b".repeat(64);
        let file_input = CreateFile {
            hash_sha256: hash.clone(),
            size: 100,
            content_type: "text/plain".to_string(),
        };
        File::create(&pool, &file_input).await.unwrap();

        // create_or_find should return existing file
        let (found, is_new) = File::create_or_find(&pool, &file_input).await.unwrap();
        assert!(!is_new);
        assert_eq!(found.hash_sha256.trim(), hash);
    }
}
