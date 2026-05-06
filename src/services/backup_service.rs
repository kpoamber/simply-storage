use bytes::Bytes;
use chrono::{DateTime, Utc};
use flate2::write::GzEncoder;
use flate2::Compression;
use sqlx::PgPool;
use std::io::Write;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::db::models::{BackupRecord, CreateBackupRecord, Storage};
use crate::error::{AppError, AppResult};
use crate::storage::StorageRegistry;

pub struct BackupService {
    pool: PgPool,
    registry: Arc<StorageRegistry>,
    database_url: String,
    temp_dir: Option<String>,
}

impl BackupService {
    pub fn new(
        pool: PgPool,
        registry: Arc<StorageRegistry>,
        database_url: String,
        temp_dir: Option<String>,
    ) -> Self {
        Self {
            pool,
            registry,
            database_url,
            temp_dir,
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Validate that a storage_path does not contain path traversal sequences
    /// or absolute path components (Unix or Windows).
    pub fn validate_storage_path(path: &str) -> AppResult<()> {
        if path.contains('\0') {
            return Err(AppError::BadRequest(
                "storage_path must not contain null bytes".to_string(),
            ));
        }
        if path.contains("..") {
            return Err(AppError::BadRequest(
                "storage_path must not contain '..'".to_string(),
            ));
        }
        if path.starts_with('/') || path.starts_with('\\') {
            return Err(AppError::BadRequest(
                "storage_path must be a relative path".to_string(),
            ));
        }
        // Reject Windows drive-letter paths (e.g. C:\, D:/) which Path::join
        // would treat as absolute, escaping the storage root.
        if path.contains(':') {
            return Err(AppError::BadRequest(
                "storage_path must not contain ':'".to_string(),
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

    /// Generate a backup filename with timestamp and short UUID for uniqueness.
    pub fn generate_backup_filename() -> String {
        let short_id = &Uuid::new_v4().to_string()[..8];
        format!("backup_{}_{}.sql.gz", Utc::now().format("%Y%m%d_%H%M%S"), short_id)
    }

    /// Verify that a storage backend is registered and available for use.
    /// Returns an error if the backend is not loaded (e.g. storage disabled or deleted).
    pub async fn validate_storage_available(&self, storage_id: &Uuid) -> AppResult<()> {
        self.registry.get(storage_id).await.map_err(|_| {
            AppError::BadRequest(format!(
                "Storage backend {} is not available. Ensure the storage exists and is enabled.",
                storage_id
            ))
        })?;
        Ok(())
    }

    /// Create a database backup, compress it, and upload to storage.
    ///
    /// Always creates a BackupRecord, even on early validation failures, so the
    /// caller (e.g. BackupWorker) does not need separate sentinel-record logic.
    pub async fn create_backup(
        &self,
        config_id: Option<Uuid>,
        config_name: Option<String>,
        storage_id: Uuid,
        storage_path: &str,
    ) -> AppResult<BackupRecord> {
        // Validate storage backend is available before starting execution.
        // On failure, create a "failed" record immediately (never a doomed
        // "running" record) so that is_backup_due() advances its baseline.
        if let Err(e) = self.validate_storage_available(&storage_id).await {
            let record = BackupRecord::create(
                &self.pool,
                &CreateBackupRecord {
                    config_id,
                    config_name,
                    storage_id,
                    file_path: String::new(),
                },
            )
            .await?;
            let _ = BackupRecord::mark_failed(
                &self.pool,
                record.id,
                &format!("Pre-execution failure: {}", e),
            )
            .await;
            return Err(e);
        }

        let filename = Self::generate_backup_filename();
        let full_path = Self::build_upload_path(storage_path, &filename);

        // Create record with "running" status
        let record = BackupRecord::create(
            &self.pool,
            &CreateBackupRecord {
                config_id,
                config_name,
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

    /// Run pg_dump, compress the output via temp files, and upload to storage.
    ///
    /// Uses temp files to avoid buffering the entire raw dump and its compressed
    /// copy in memory simultaneously. Only the final compressed file is read into
    /// memory for upload.
    async fn execute_backup(&self, upload_path: &str, storage_id: Uuid) -> AppResult<i64> {
        // Write pg_dump output to a temp file instead of buffering in memory.
        // Use configured temp_dir if set, otherwise fall back to OS default.
        let dump_file = if let Some(ref dir) = self.temp_dir {
            let dir_path = std::path::Path::new(dir);
            if !dir_path.exists() {
                std::fs::create_dir_all(dir_path)
                    .map_err(|e| AppError::Internal(format!("Failed to create temp dir {}: {}", dir, e)))?;
            }
            tempfile::NamedTempFile::new_in(dir)
        } else {
            tempfile::NamedTempFile::new()
        }
        .map_err(|e| AppError::Internal(format!("Failed to create temp file: {}", e)))?;
        // Close the file handle immediately so pg_dump can write to the path
        // on Windows, which enforces mandatory file locks. The TempPath still
        // auto-deletes the file on drop.
        let dump_temp = dump_file.into_temp_path();
        let dump_path = dump_temp.to_path_buf();

        // Parse the database URL so we can pass the password via PGPASSWORD
        // env var instead of on the command line (which is visible in `ps`).
        let parsed_url = url::Url::parse(&self.database_url)
            .map_err(|e| AppError::Internal(format!("Invalid database URL: {}", e)))?;

        let mut cmd = tokio::process::Command::new("pg_dump");
        cmd.arg("--no-owner")
            .arg("--no-acl")
            .arg("--file")
            .arg(&dump_path);

        if let Some(password) = parsed_url.password() {
            cmd.env("PGPASSWORD", password);
        }
        if let Some(host) = parsed_url.host_str() {
            cmd.arg("--host").arg(host);
        }
        if let Some(port) = parsed_url.port() {
            cmd.arg("--port").arg(port.to_string());
        }
        let username = parsed_url.username();
        if !username.is_empty() {
            cmd.arg("--username").arg(username);
        }
        let dbname = parsed_url.path().trim_start_matches('/');
        if !dbname.is_empty() {
            cmd.arg("--dbname").arg(dbname);
        }

        let output = cmd
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to execute pg_dump: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!(
                status = %output.status,
                stderr = %stderr,
                "pg_dump failed"
            );
            return Err(AppError::Internal(format!(
                "pg_dump failed (exit {}): {}",
                output.status,
                stderr.chars().take(500).collect::<String>(),
            )));
        }

        // Compress the dump file - read in chunks to limit memory usage.
        // Only the compressed result needs to be in memory for upload.
        let compressed_file = if let Some(ref dir) = self.temp_dir {
            tempfile::NamedTempFile::new_in(dir)
        } else {
            tempfile::NamedTempFile::new()
        }
        .map_err(|e| AppError::Internal(format!("Failed to create temp file: {}", e)))?;
        // Close the handle for Windows compatibility before creating a new
        // writer at the same path.
        let compressed_temp = compressed_file.into_temp_path();
        let compressed_path = compressed_temp.to_path_buf();

        // Read raw dump and compress into the compressed temp file
        {
            let mut reader: tokio::fs::File = tokio::fs::File::open(&dump_path).await?;
            let compressed_std = std::fs::File::create(&compressed_path)
                .map_err(|e| AppError::Internal(format!("Failed to create compressed file: {}", e)))?;
            let mut encoder = GzEncoder::new(compressed_std, Compression::default());
            let mut buf = vec![0u8; 64 * 1024]; // 64 KB chunks
            loop {
                let n = reader.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                encoder.write_all(&buf[..n])?;
            }
            encoder.finish()?;
        }
        // Drop the raw dump temp file early to free disk space
        drop(dump_temp);

        // Read compressed file into memory for upload
        let mut compressed_reader: tokio::fs::File = tokio::fs::File::open(&compressed_path).await?;
        let file_size = tokio::fs::metadata(&compressed_path).await?.len() as i64;
        let mut compressed = Vec::with_capacity(file_size as usize);
        compressed_reader.read_to_end(&mut compressed).await?;
        drop(compressed_temp);

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
        if retention_count < 1 {
            return Ok(0);
        }
        let old_backups =
            BackupRecord::list_oldest_completed_by_config(&self.pool, config_id, retention_count)
                .await?;

        let mut deleted = 0u32;
        for backup in &old_backups {
            if let Err(e) = self.delete_from_storage(backup).await {
                tracing::warn!(
                    backup_id = %backup.id,
                    error = %e,
                    "Failed to delete backup file from storage during cleanup, skipping DB record deletion"
                );
                // Skip DB record deletion to avoid orphaning the file.
                // The record will be retried on the next cleanup cycle.
                continue;
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
    ///
    /// Deletes the file from storage first, then removes the DB record.
    /// If the storage is disabled, returns an error so the admin can re-enable
    /// it to properly clean up the file. If the storage has been completely
    /// deleted from the system, proceeds with DB-only cleanup since the file
    /// is already unreachable.
    pub async fn delete_backup(&self, backup_id: Uuid) -> AppResult<()> {
        let record = BackupRecord::find_by_id(&self.pool, backup_id).await?;

        // Sentinel rows (pre-execution failures) have empty file_path and never
        // uploaded a file, so skip storage deletion entirely for them.
        if !record.file_path.is_empty() {
            match self.registry.get(&record.storage_id).await {
                Ok(backend) => {
                    backend.delete(&record.file_path).await?;
                }
                Err(_) => {
                    // Backend not loaded. Check if the storage still exists in DB.
                    // Distinguish NotFound (storage deleted) from transient DB errors
                    // to avoid orphaning backup files on temporary failures.
                    match Storage::find_by_id(&self.pool, record.storage_id).await {
                        Ok(storage) => {
                            // Storage exists but is disabled/unregistered. Require
                            // the admin to re-enable it to avoid orphaning the file.
                            return Err(AppError::BadRequest(format!(
                                "Storage '{}' is disabled. Re-enable it before deleting backups to avoid orphaning files.",
                                storage.name
                            )));
                        }
                        Err(AppError::NotFound(_)) => {
                            // Storage has been completely removed. The file is
                            // already unreachable; proceed with DB cleanup.
                            tracing::warn!(
                                backup_id = %backup_id,
                                storage_id = %record.storage_id,
                                file_path = %record.file_path,
                                "Storage no longer exists, deleting DB record only. Backup file may be orphaned."
                            );
                        }
                        Err(e) => {
                            // Transient DB error (connection issue, etc.). Do NOT
                            // proceed with DB-only cleanup as we can't confirm the
                            // storage was actually deleted.
                            return Err(e);
                        }
                    }
                }
            }
        }

        BackupRecord::delete(&self.pool, backup_id).await?;
        Ok(())
    }

    /// Delete all backup files and records for a config.
    /// Used when deleting a config with delete_backups=true.
    /// Only deletes DB records whose storage files were successfully removed
    /// (or that have no file) to avoid orphaning files on transient failures.
    pub async fn delete_all_backups_for_config(&self, config_id: Uuid) -> AppResult<u64> {
        let records = BackupRecord::list_all_by_config(&self.pool, config_id).await?;

        let mut deleted = 0u64;
        for record in &records {
            if record.file_path.is_empty() {
                BackupRecord::delete(&self.pool, record.id).await?;
                deleted += 1;
                continue;
            }
            if let Err(e) = self.delete_from_storage(record).await {
                tracing::warn!(
                    backup_id = %record.id,
                    file_path = %record.file_path,
                    error = %e,
                    "Failed to delete backup file from storage during config cleanup, keeping DB record"
                );
                continue;
            }
            BackupRecord::delete(&self.pool, record.id).await?;
            deleted += 1;
        }

        Ok(deleted)
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

    #[test]
    fn test_validate_storage_path_null_byte() {
        let result = BackupService::validate_storage_path("backups/\0evil");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_storage_path_windows_absolute() {
        assert!(BackupService::validate_storage_path("C:\\temp\\backup.sql.gz").is_err());
        assert!(BackupService::validate_storage_path("C:/temp/backup.sql.gz").is_err());
        assert!(BackupService::validate_storage_path("D:\\backup").is_err());
        assert!(BackupService::validate_storage_path("\\\\server\\share").is_err());
    }
}
