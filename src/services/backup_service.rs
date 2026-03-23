use bytes::Bytes;
use chrono::{DateTime, Utc};
use flate2::write::GzEncoder;
use flate2::Compression;
use sqlx::PgPool;
use std::io::Write;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::models::{BackupRecord, CreateBackupRecord};
use crate::error::{AppError, AppResult};
use crate::storage::StorageRegistry;

pub struct BackupService {
    pool: PgPool,
    registry: Arc<StorageRegistry>,
    database_url: String,
}

impl BackupService {
    pub fn new(
        pool: PgPool,
        registry: Arc<StorageRegistry>,
        database_url: String,
    ) -> Self {
        Self {
            pool,
            registry,
            database_url,
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Validate that a storage_path does not contain path traversal sequences.
    pub fn validate_storage_path(path: &str) -> AppResult<()> {
        if path.contains("..") {
            return Err(AppError::BadRequest(
                "storage_path must not contain '..'".to_string(),
            ));
        }
        if path.starts_with('/') {
            return Err(AppError::BadRequest(
                "storage_path must be a relative path".to_string(),
            ));
        }
        Ok(())
    }

    /// Build the upload path from storage_path and filename.
    pub fn build_upload_path(storage_path: &str, filename: &str) -> String {
        if storage_path.is_empty() {
            filename.to_string()
        } else {
            format!("{}/{}", storage_path.trim_end_matches('/'), filename)
        }
    }

    /// Generate a backup filename with the current timestamp.
    pub fn generate_backup_filename() -> String {
        format!("backup_{}.sql.gz", Utc::now().format("%Y%m%d_%H%M%S"))
    }

    /// Create a database backup, compress it, and upload to storage.
    pub async fn create_backup(
        &self,
        config_id: Option<Uuid>,
        storage_id: Uuid,
        storage_path: &str,
    ) -> AppResult<BackupRecord> {
        let filename = Self::generate_backup_filename();
        let full_path = Self::build_upload_path(storage_path, &filename);

        // Create record with "running" status
        let record = BackupRecord::create(
            &self.pool,
            &CreateBackupRecord {
                config_id,
                storage_id,
                file_path: full_path.clone(),
            },
        )
        .await?;

        match self.execute_backup(&full_path, storage_id).await {
            Ok(file_size) => {
                let updated = BackupRecord::mark_completed(
                    &self.pool,
                    record.id,
                    file_size,
                    &full_path,
                )
                .await?;
                Ok(updated)
            }
            Err(e) => {
                let _ = BackupRecord::mark_failed(
                    &self.pool,
                    record.id,
                    &e.to_string(),
                )
                .await;
                Err(e)
            }
        }
    }

    /// Execute the backup for an existing record: run pg_dump, upload, update status.
    /// Used by the trigger API to run backups in background.
    pub async fn execute_backup_and_update(
        &self,
        record_id: Uuid,
        upload_path: &str,
        storage_id: Uuid,
    ) -> AppResult<BackupRecord> {
        match self.execute_backup(upload_path, storage_id).await {
            Ok(file_size) => {
                let updated = BackupRecord::mark_completed(
                    &self.pool,
                    record_id,
                    file_size,
                    upload_path,
                )
                .await?;
                Ok(updated)
            }
            Err(e) => {
                let _ = BackupRecord::mark_failed(
                    &self.pool,
                    record_id,
                    &e.to_string(),
                )
                .await;
                Err(e)
            }
        }
    }

    /// Run pg_dump, compress the output, and upload to storage.
    async fn execute_backup(&self, upload_path: &str, storage_id: Uuid) -> AppResult<i64> {
        let output = tokio::process::Command::new("pg_dump")
            .arg("--no-owner")
            .arg("--no-acl")
            .arg(&self.database_url)
            .output()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to execute pg_dump: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!(stderr = %stderr, "pg_dump failed");
            return Err(AppError::Internal(
                "pg_dump failed, see server logs for details".to_string(),
            ));
        }

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&output.stdout)?;
        let compressed = encoder.finish()?;
        let file_size = compressed.len() as i64;

        let backend = self.registry.get(&storage_id).await?;
        backend.upload(upload_path, Bytes::from(compressed)).await?;

        tracing::info!(
            path = upload_path,
            size = file_size,
            "Database backup uploaded to storage"
        );

        Ok(file_size)
    }

    /// Delete old backups beyond the retention count for a config.
    pub async fn cleanup_old_backups(
        &self,
        config_id: Uuid,
        retention_count: i32,
    ) -> AppResult<u32> {
        let old_backups =
            BackupRecord::list_oldest_completed_by_config(&self.pool, config_id, retention_count)
                .await?;

        let mut deleted = 0u32;
        for backup in &old_backups {
            if let Err(e) = self.delete_from_storage(backup).await {
                tracing::warn!(
                    backup_id = %backup.id,
                    error = %e,
                    "Failed to delete backup file from storage during cleanup"
                );
            }
            BackupRecord::delete(&self.pool, backup.id).await?;
            deleted += 1;
        }

        if deleted > 0 {
            tracing::info!(
                config_id = %config_id,
                deleted_count = deleted,
                "Cleaned up old backups"
            );
        }

        Ok(deleted)
    }

    /// Delete a single backup from storage and database.
    pub async fn delete_backup(&self, backup_id: Uuid) -> AppResult<()> {
        let record = BackupRecord::find_by_id(&self.pool, backup_id).await?;

        if let Err(e) = self.delete_from_storage(&record).await {
            tracing::warn!(
                backup_id = %backup_id,
                error = %e,
                "Failed to delete backup file from storage"
            );
        }

        BackupRecord::delete(&self.pool, backup_id).await?;
        Ok(())
    }

    async fn delete_from_storage(&self, record: &BackupRecord) -> AppResult<()> {
        let backend = self.registry.get(&record.storage_id).await?;
        backend.delete(&record.file_path).await
    }

    /// Parse a cron expression and return the next scheduled run time.
    /// Uses 7-field cron format: sec min hour dom month dow year
    pub fn get_next_run_time(cron_expr: &str) -> Option<DateTime<Utc>> {
        use cron::Schedule;
        use std::str::FromStr;

        let schedule = Schedule::from_str(cron_expr).ok()?;
        schedule.upcoming(Utc).next()
    }

    /// Validate a cron expression (7-field format: sec min hour dom month dow year).
    pub fn validate_cron(cron_expr: &str) -> AppResult<()> {
        use cron::Schedule;
        use std::str::FromStr;

        Schedule::from_str(cron_expr)
            .map_err(|e| AppError::BadRequest(format!("Invalid cron expression: {}", e)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn test_get_next_run_time_valid_cron() {
        let next = BackupService::get_next_run_time("0 * * * * * *");
        assert!(next.is_some());
        assert!(next.unwrap() > Utc::now());
    }

    #[test]
    fn test_get_next_run_time_daily_at_2am() {
        let next = BackupService::get_next_run_time("0 0 2 * * * *");
        assert!(next.is_some());
        let next = next.unwrap();
        assert_eq!(next.hour(), 2);
        assert_eq!(next.minute(), 0);
    }

    #[test]
    fn test_get_next_run_time_invalid_cron() {
        assert!(BackupService::get_next_run_time("not a cron").is_none());
    }

    #[test]
    fn test_get_next_run_time_empty_string() {
        assert!(BackupService::get_next_run_time("").is_none());
    }

    #[test]
    fn test_validate_cron_valid() {
        assert!(BackupService::validate_cron("0 0 2 * * * *").is_ok());
        assert!(BackupService::validate_cron("0 30 1 * * Mon *").is_ok());
        assert!(BackupService::validate_cron("0 0 */6 * * * *").is_ok());
    }

    #[test]
    fn test_validate_cron_invalid() {
        let result = BackupService::validate_cron("not a cron");
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::BadRequest(msg) => {
                assert!(msg.contains("Invalid cron expression"));
            }
            other => panic!("Expected BadRequest, got {:?}", other),
        }
    }

    #[test]
    fn test_validate_cron_empty() {
        assert!(BackupService::validate_cron("").is_err());
    }

    #[test]
    fn test_weekly_cron_schedule() {
        let next = BackupService::get_next_run_time("0 0 0 * * Sun *");
        assert!(next.is_some());
        let next = next.unwrap();
        assert_eq!(next.hour(), 0);
        assert_eq!(next.minute(), 0);
        assert_eq!(next.weekday().num_days_from_sunday(), 0);
    }

    #[test]
    fn test_generate_backup_filename() {
        let filename = BackupService::generate_backup_filename();
        assert!(filename.starts_with("backup_"));
        assert!(filename.ends_with(".sql.gz"));
        assert!(filename.len() > "backup_.sql.gz".len());
    }

    #[test]
    fn test_build_upload_path_with_storage_path() {
        let path = BackupService::build_upload_path("backups/daily", "backup_20260323.sql.gz");
        assert_eq!(path, "backups/daily/backup_20260323.sql.gz");
    }

    #[test]
    fn test_build_upload_path_with_trailing_slash() {
        let path = BackupService::build_upload_path("backups/", "backup_20260323.sql.gz");
        assert_eq!(path, "backups/backup_20260323.sql.gz");
    }

    #[test]
    fn test_build_upload_path_empty_storage_path() {
        let path = BackupService::build_upload_path("", "backup_20260323.sql.gz");
        assert_eq!(path, "backup_20260323.sql.gz");
    }

    #[test]
    fn test_validate_storage_path_valid() {
        assert!(BackupService::validate_storage_path("backups/daily").is_ok());
        assert!(BackupService::validate_storage_path("backups").is_ok());
        assert!(BackupService::validate_storage_path("").is_ok());
    }

    #[test]
    fn test_validate_storage_path_traversal() {
        let result = BackupService::validate_storage_path("../etc/passwd");
        assert!(result.is_err());
        let result = BackupService::validate_storage_path("backups/../../etc");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_storage_path_absolute() {
        let result = BackupService::validate_storage_path("/absolute/path");
        assert!(result.is_err());
    }
}
