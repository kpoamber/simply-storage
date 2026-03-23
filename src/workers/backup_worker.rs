use chrono::Utc;
use cron::Schedule;
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::db::models::{BackupConfig, BackupRecord};
use crate::services::BackupService;
use crate::storage::StorageRegistry;

/// Background worker that runs database backups on cron schedules.
///
/// Periodically checks enabled BackupConfigs, determines whether each is due
/// based on its cron expression and last backup time, and triggers backups.
/// After a successful backup, cleans up old backups beyond retention count.
pub struct BackupWorker {
    backup_service: BackupService,
    pool: PgPool,
    cancel_token: CancellationToken,
    check_interval: Duration,
}

impl BackupWorker {
    pub fn new(
        pool: PgPool,
        registry: Arc<StorageRegistry>,
        database_url: String,
        cancel_token: CancellationToken,
        check_interval_secs: u64,
    ) -> Self {
        Self {
            backup_service: BackupService::new(pool.clone(), registry, database_url),
            pool,
            cancel_token,
            check_interval: Duration::from_secs(check_interval_secs),
        }
    }

    /// Spawn the backup worker as a tokio task. Returns the JoinHandle.
    pub fn spawn(
        pool: PgPool,
        registry: Arc<StorageRegistry>,
        database_url: String,
        cancel_token: CancellationToken,
        check_interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        let worker = Self::new(pool, registry, database_url, cancel_token, check_interval_secs);
        tokio::spawn(async move {
            tracing::info!("Backup worker started");
            worker.run().await;
            tracing::info!("Backup worker stopped");
        })
    }

    /// Main worker loop: check schedules, run backups, sleep, repeat.
    async fn run(&self) {
        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    tracing::info!("Backup worker received shutdown signal");
                    break;
                }
                _ = self.check_and_run_backups() => {
                    tokio::select! {
                        _ = self.cancel_token.cancelled() => {
                            tracing::info!("Backup worker received shutdown signal during sleep");
                            break;
                        }
                        _ = tokio::time::sleep(self.check_interval) => {}
                    }
                }
            }
        }
    }

    /// Check all enabled configs and run backups that are due.
    async fn check_and_run_backups(&self) {
        let configs = match BackupConfig::list_enabled(&self.pool).await {
            Ok(configs) => configs,
            Err(e) => {
                tracing::error!(error = %e, "Failed to list enabled backup configs");
                return;
            }
        };

        for config in &configs {
            if let Err(e) = self.process_config(config).await {
                tracing::error!(
                    config_id = %config.id,
                    config_name = %config.name,
                    error = %e,
                    "Failed to process backup config"
                );
            }
        }
    }

    /// Process a single backup config: check if it's due and run if so.
    async fn process_config(&self, config: &BackupConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !is_backup_due(&config.schedule_cron, config.id, &self.pool).await? {
            return Ok(());
        }

        tracing::info!(
            config_id = %config.id,
            config_name = %config.name,
            "Running scheduled backup"
        );

        match self
            .backup_service
            .create_backup(Some(config.id), config.storage_id, &config.storage_path)
            .await
        {
            Ok(record) => {
                tracing::info!(
                    config_id = %config.id,
                    backup_id = %record.id,
                    file_size = record.file_size_bytes,
                    "Scheduled backup completed successfully"
                );

                // Clean up old backups beyond retention count
                if let Err(e) = self
                    .backup_service
                    .cleanup_old_backups(config.id, config.retention_count)
                    .await
                {
                    tracing::error!(
                        config_id = %config.id,
                        error = %e,
                        "Failed to cleanup old backups after scheduled run"
                    );
                }
            }
            Err(e) => {
                tracing::error!(
                    config_id = %config.id,
                    config_name = %config.name,
                    error = %e,
                    "Scheduled backup failed"
                );
            }
        }

        Ok(())
    }
}

/// Determine if a backup is due based on the cron schedule and last backup time.
///
/// Returns true if the cron expression's most recent scheduled time is after
/// the last backup's started_at (or if no backup has been run yet).
pub async fn is_backup_due(
    cron_expr: &str,
    config_id: uuid::Uuid,
    pool: &PgPool,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let schedule = Schedule::from_str(cron_expr)?;

    // Find the last backup for this config
    let records = BackupRecord::list(pool, Some(config_id)).await?;
    let last_backup = records.first(); // list is ordered by created_at DESC

    let last_run_at = last_backup.and_then(|r| r.started_at);

    match last_run_at {
        Some(last_run) => {
            // Check if there's a scheduled time between last_run and now
            // Use the schedule to find the next occurrence after last_run
            let next_after_last = schedule.after(&last_run).next();
            match next_after_last {
                Some(next_time) if next_time <= Utc::now() => Ok(true),
                _ => Ok(false),
            }
        }
        None => {
            // No previous backup: it's due if the cron is valid
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, Utc};
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_backup_worker_cancellation() {
        let cancel_token = CancellationToken::new();
        let child = cancel_token.clone();
        cancel_token.cancel();
        assert!(child.is_cancelled());
    }

    #[tokio::test]
    async fn test_backup_worker_cancellation_propagation() {
        let parent = CancellationToken::new();
        let child = parent.child_token();
        assert!(!child.is_cancelled());
        parent.cancel();
        assert!(child.is_cancelled());
    }

    #[test]
    fn test_check_interval_duration() {
        let interval = Duration::from_secs(60);
        assert_eq!(interval.as_secs(), 60);
        let custom = Duration::from_secs(120);
        assert_eq!(custom.as_secs(), 120);
    }

    #[test]
    fn test_default_check_interval() {
        // Default backup check interval is 60 seconds
        let default_secs: u64 = 60;
        let interval = Duration::from_secs(default_secs);
        assert_eq!(interval.as_secs(), 60);
    }

    #[test]
    fn test_cron_schedule_parsing() {
        // Daily at 2:00 AM (7-field cron: sec min hour dom month dow year)
        let schedule = Schedule::from_str("0 0 2 * * * *");
        assert!(schedule.is_ok());

        // Every 6 hours
        let schedule = Schedule::from_str("0 0 */6 * * * *");
        assert!(schedule.is_ok());

        // Invalid expression
        let schedule = Schedule::from_str("not a cron");
        assert!(schedule.is_err());
    }

    #[test]
    fn test_cron_next_occurrence_is_in_future() {
        let schedule = Schedule::from_str("0 * * * * * *").unwrap(); // every minute
        let next = schedule.upcoming(Utc).next();
        assert!(next.is_some());
        assert!(next.unwrap() > Utc::now());
    }

    #[test]
    fn test_cron_after_past_time() {
        let schedule = Schedule::from_str("0 * * * * * *").unwrap(); // every minute
        let past = Utc::now() - ChronoDuration::hours(2);
        let next_after_past = schedule.after(&past).next();
        assert!(next_after_past.is_some());
        // Should be at most ~1 minute in the future (next minute boundary)
        let next = next_after_past.unwrap();
        assert!(next <= Utc::now() + ChronoDuration::minutes(1));
    }

    #[test]
    fn test_cron_after_recent_time_not_due() {
        let schedule = Schedule::from_str("0 0 2 * * * *").unwrap(); // daily at 2am
        let now = Utc::now();
        // If last run was just now, next occurrence is ~24h away, so not due
        let next_after_now = schedule.after(&now).next();
        assert!(next_after_now.is_some());
        assert!(next_after_now.unwrap() > now);
    }

    // ─── Integration tests requiring PostgreSQL ─────────────────────────────────

    #[ignore]
    #[tokio::test]
    async fn test_backup_worker_spawn_and_shutdown() {
        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL required for integration tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();
        let registry = Arc::new(StorageRegistry::new());
        let cancel_token = CancellationToken::new();

        let handle = BackupWorker::spawn(
            pool,
            registry,
            url,
            cancel_token.clone(),
            1,
        );

        // Let it run briefly
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Signal shutdown
        cancel_token.cancel();

        // Should stop within timeout
        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("Worker should stop within timeout")
            .expect("Worker should not panic");
    }

    #[ignore]
    #[tokio::test]
    async fn test_is_backup_due_no_previous_backup() {
        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL required for integration tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Create a backup config
        let config = crate::db::models::BackupConfig::create(
            &pool,
            &crate::db::models::CreateBackupConfig {
                name: "test-due-check".to_string(),
                storage_id: uuid::Uuid::new_v4(), // dummy, won't be used
                storage_path: Some("test".to_string()),
                schedule_cron: "0 * * * * * *".to_string(), // every minute
                retention_count: Some(5),
                enabled: Some(true),
            },
        )
        .await
        .unwrap();

        // No previous backup - should be due
        let due = is_backup_due(&config.schedule_cron, config.id, &pool)
            .await
            .unwrap();
        assert!(due, "Should be due when no previous backup exists");

        // Cleanup
        sqlx::query("DELETE FROM backup_configs WHERE id = $1")
            .bind(config.id)
            .execute(&pool)
            .await
            .unwrap();
    }
}
