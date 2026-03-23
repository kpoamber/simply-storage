use chrono::Utc;
use cron::Schedule;
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::db::models::{BackupConfig, BackupRecord};
use crate::services::BackupService;

/// Background worker that runs database backups on cron schedules.
///
/// Periodically checks enabled BackupConfigs, determines whether each is due
/// based on its cron expression and last backup time, and triggers backups.
/// After a successful backup, cleans up old backups beyond retention count.
///
/// Uses PostgreSQL advisory locks to prevent duplicate backups when running
/// multiple app instances.
pub struct BackupWorker {
    backup_service: Arc<BackupService>,
    cancel_token: CancellationToken,
    check_interval: Duration,
}

impl BackupWorker {
    pub fn new(
        backup_service: Arc<BackupService>,
        cancel_token: CancellationToken,
        check_interval_secs: u64,
    ) -> Self {
        Self {
            backup_service,
            cancel_token,
            check_interval: Duration::from_secs(check_interval_secs),
        }
    }

    /// Spawn the backup worker as a tokio task. Returns the JoinHandle.
    pub fn spawn(
        backup_service: Arc<BackupService>,
        cancel_token: CancellationToken,
        check_interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        let worker = Self::new(backup_service, cancel_token, check_interval_secs);
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
        let pool = self.backup_service.pool();
        let configs = match BackupConfig::list_enabled(pool).await {
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
    /// Uses a PostgreSQL session-level advisory lock on a dedicated connection
    /// to prevent duplicate backups across replicas. Both lock and unlock run
    /// on the same connection, ensuring proper release.
    async fn process_config(&self, config: &BackupConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.backup_service.pool();

        if !is_backup_due(&config.schedule_cron, config.id, pool).await? {
            return Ok(());
        }

        // Use session-level advisory lock to prevent duplicate backups.
        // Key derived from config UUID bytes to get a unique lock per config.
        let id_bytes = config.id.as_bytes();
        let key1 = i32::from_le_bytes([id_bytes[0], id_bytes[1], id_bytes[2], id_bytes[3]]);
        let key2 = i32::from_le_bytes([id_bytes[4], id_bytes[5], id_bytes[6], id_bytes[7]]);

        // Acquire a dedicated connection so that lock and unlock happen on the
        // same session. Using the pool directly would risk acquiring/releasing
        // on different connections, making the lock ineffective.
        let mut conn = pool.acquire().await?;
        let locked: (bool,) = sqlx::query_as("SELECT pg_try_advisory_lock($1, $2)")
            .bind(key1)
            .bind(key2)
            .fetch_one(&mut *conn)
            .await?;

        if !locked.0 {
            tracing::debug!(
                config_id = %config.id,
                "Another instance is already running this backup, skipping"
            );
            return Ok(());
        }

        tracing::info!(
            config_id = %config.id,
            config_name = %config.name,
            "Running scheduled backup"
        );

        let result = self
            .backup_service
            .create_backup(Some(config.id), config.storage_id, &config.storage_path)
            .await;

        // Always release the advisory lock on the same connection that acquired it.
        let _ = sqlx::query("SELECT pg_advisory_unlock($1, $2)")
            .bind(key1)
            .bind(key2)
            .execute(&mut *conn)
            .await;

        match result {
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

/// Maximum time a backup can stay in "running" status before being considered stale.
const STALE_RUNNING_THRESHOLD_SECS: i64 = 3600; // 1 hour

/// Determine if a backup is due based on the cron schedule and last backup time.
///
/// Returns true if the cron expression's most recent scheduled time is after
/// the last successful/running backup's started_at (or if no backup has been run yet).
/// Failed backups are ignored so that retries happen on the next check cycle.
/// Running backups older than STALE_RUNNING_THRESHOLD_SECS are marked as failed
/// and ignored, preventing orphaned records from blocking future backups.
pub async fn is_backup_due(
    cron_expr: &str,
    config_id: uuid::Uuid,
    pool: &PgPool,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let schedule = Schedule::from_str(cron_expr)?;

    // Find the last completed or running backup for this config (LIMIT 1)
    let last_backup = BackupRecord::find_latest_successful_by_config(pool, config_id).await?;

    // Check for stale "running" records and mark them as failed
    if let Some(ref record) = last_backup {
        if record.status == "running" {
            if let Some(started) = record.started_at {
                let elapsed = Utc::now() - started;
                if elapsed.num_seconds() > STALE_RUNNING_THRESHOLD_SECS {
                    tracing::warn!(
                        backup_id = %record.id,
                        config_id = %config_id,
                        started_at = %started,
                        "Marking stale running backup as failed (exceeded {} seconds)",
                        STALE_RUNNING_THRESHOLD_SECS
                    );
                    let _ = BackupRecord::mark_failed(
                        pool,
                        record.id,
                        "Marked as failed: exceeded maximum running time (stale)",
                    )
                    .await;
                    // After marking stale record as failed, this backup is now due
                    return Ok(true);
                }
            }
        }
    }

    let last_run_at = last_backup.as_ref().and_then(|r| r.started_at);

    match last_run_at {
        Some(last_run) => {
            // Check if there's a scheduled time between last_run and now
            let next_after_last = schedule.after(&last_run).next();
            match next_after_last {
                Some(next_time) if next_time <= Utc::now() => Ok(true),
                _ => Ok(false),
            }
        }
        None => {
            // No previous successful backup: it's due if the cron is valid
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // ─── Integration tests requiring PostgreSQL ─────────────────────────────────

    #[ignore]
    #[tokio::test]
    async fn test_backup_worker_spawn_and_shutdown() {
        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL required for integration tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();
        let registry = Arc::new(crate::storage::StorageRegistry::new());
        let cancel_token = CancellationToken::new();

        let service = Arc::new(BackupService::new(pool, registry, url));
        let handle = BackupWorker::spawn(
            service,
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
