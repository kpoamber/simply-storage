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

    /// Create a database backup, compress it, and upload to storage.
    pub async fn create_backup(
        &self,
        config_id: Option<Uuid>,
        storage_id: Uuid,
        storage_path: &str,
    ) -> AppResult<BackupRecord> {
        let filename = format!("backup_{}.sql.gz", Utc::now().format("%Y%m%d_%H%M%S"));
        let full_path = if storage_path.is_empty() {
            filename.clone()
        } else {
            format!("{}/{}", storage_path.trim_end_matches('/'), filename)
        };

        // Create record with "running" status
        let record = BackupRecord::create(
            &self.pool,
            &CreateBackupRecord {
                config_id,
                storage_id,
                file_path: full_path.clone(),
                status: "running".to_string(),
            },
        )
        .await?;

        match self.execute_backup(&full_path, storage_id).await {
            Ok(file_size) => {
                let updated = BackupRecord::update_status(
                    &self.pool,
                    record.id,
                    "completed",
                    None,
                    Some(file_size),
                    Some(&full_path),
                )
                .await?;
                Ok(updated)
            }
            Err(e) => {
                let _ = BackupRecord::update_status(
                    &self.pool,
                    record.id,
                    "failed",
                    Some(&e.to_string()),
                    None,
                    None,
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
            return Err(AppError::Internal(format!("pg_dump failed: {}", stderr)));
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
        // Every minute
        let next = BackupService::get_next_run_time("0 * * * * * *");
        assert!(next.is_some());
        let next = next.unwrap();
        assert!(next > Utc::now());
    }

    #[test]
    fn test_get_next_run_time_daily_at_2am() {
        // Daily at 2:00 AM
        let next = BackupService::get_next_run_time("0 0 2 * * * *");
        assert!(next.is_some());
        let next = next.unwrap();
        assert_eq!(next.hour(), 2);
        assert_eq!(next.minute(), 0);
    }

    #[test]
    fn test_get_next_run_time_invalid_cron() {
        let next = BackupService::get_next_run_time("not a cron");
        assert!(next.is_none());
    }

    #[test]
    fn test_get_next_run_time_empty_string() {
        let next = BackupService::get_next_run_time("");
        assert!(next.is_none());
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
    fn test_backup_filename_format() {
        let now = Utc::now();
        let filename = format!("backup_{}.sql.gz", now.format("%Y%m%d_%H%M%S"));
        assert!(filename.starts_with("backup_"));
        assert!(filename.ends_with(".sql.gz"));
    }

    #[test]
    fn test_full_path_with_storage_path() {
        let storage_path = "backups/daily";
        let filename = "backup_20260323_020000.sql.gz";
        let full_path = format!("{}/{}", storage_path.trim_end_matches('/'), filename);
        assert_eq!(full_path, "backups/daily/backup_20260323_020000.sql.gz");
    }

    #[test]
    fn test_full_path_with_trailing_slash() {
        let storage_path = "backups/";
        let filename = "backup_20260323_020000.sql.gz";
        let full_path = format!("{}/{}", storage_path.trim_end_matches('/'), filename);
        assert_eq!(full_path, "backups/backup_20260323_020000.sql.gz");
    }

    #[test]
    fn test_full_path_empty_storage_path() {
        let storage_path = "";
        let filename = "backup_20260323_020000.sql.gz";
        let full_path = if storage_path.is_empty() {
            filename.to_string()
        } else {
            format!("{}/{}", storage_path.trim_end_matches('/'), filename)
        };
        assert_eq!(full_path, "backup_20260323_020000.sql.gz");
    }

    #[test]
    fn test_gzip_compression() {
        let data = b"CREATE TABLE test (id INT); INSERT INTO test VALUES (1);";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        let compressed = encoder.finish().unwrap();

        // Compressed data should be non-empty
        assert!(!compressed.is_empty());
        // Gzip magic number
        assert_eq!(compressed[0], 0x1f);
        assert_eq!(compressed[1], 0x8b);

        // Decompress and verify
        use flate2::read::GzDecoder;
        use std::io::Read;
        let mut decoder = GzDecoder::new(&compressed[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_next_run_time_is_in_future() {
        let next = BackupService::get_next_run_time("0 * * * * * *");
        assert!(next.is_some());
        assert!(next.unwrap() > Utc::now());
    }

    #[test]
    fn test_weekly_cron_schedule() {
        // Every Sunday at midnight
        let next = BackupService::get_next_run_time("0 0 0 * * Sun *");
        assert!(next.is_some());
        let next = next.unwrap();
        assert_eq!(next.hour(), 0);
        assert_eq!(next.minute(), 0);
        // chrono weekday: Sun = 6 (Mon=0..Sun=6), but Datelike::weekday().num_days_from_sunday() = 0
        assert_eq!(next.weekday().num_days_from_sunday(), 0);
    }
}
