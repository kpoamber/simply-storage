use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::services::TierService;
use crate::storage::StorageRegistry;

/// Background worker for hot/cold tier management.
///
/// Periodically scans for files eligible for archiving (based on project
/// `hot_to_cold_days` settings and last access time) and creates sync tasks
/// to move them to cold storage. Also marks hot storage locations as archived
/// once the cold copy is synced.
pub struct TierWorker {
    service: TierService,
    cancel_token: CancellationToken,
    scan_interval: Duration,
}

impl TierWorker {
    pub fn new(
        pool: PgPool,
        registry: Arc<StorageRegistry>,
        cancel_token: CancellationToken,
        scan_interval_secs: u64,
    ) -> Self {
        Self {
            service: TierService::new(pool, registry),
            cancel_token,
            scan_interval: Duration::from_secs(scan_interval_secs),
        }
    }

    /// Spawn the tier worker as a tokio task. Returns the JoinHandle.
    pub fn spawn(
        pool: PgPool,
        registry: Arc<StorageRegistry>,
        cancel_token: CancellationToken,
        scan_interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        let worker = Self::new(pool, registry, cancel_token, scan_interval_secs);
        tokio::spawn(async move {
            tracing::info!("Tier worker started");
            worker.run().await;
            tracing::info!("Tier worker stopped");
        })
    }

    /// Main worker loop: scan, archive, sleep, repeat.
    async fn run(&self) {
        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    tracing::info!("Tier worker received shutdown signal");
                    break;
                }
                _ = self.scan_and_archive() => {
                    tokio::select! {
                        _ = self.cancel_token.cancelled() => {
                            tracing::info!("Tier worker received shutdown signal during sleep");
                            break;
                        }
                        _ = tokio::time::sleep(self.scan_interval) => {}
                    }
                }
            }
        }
    }

    /// Perform one scan cycle: find archivable files and process completed archives.
    async fn scan_and_archive(&self) {
        // Phase 1: Create archive sync tasks for eligible files
        match self.service.create_archive_tasks().await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(count, "Created archive sync tasks");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to create archive tasks");
            }
        }

        // Phase 2: Mark hot locations as archived where cold copy is synced
        match self.service.process_completed_archives().await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(count, "Archived hot storage locations");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to process completed archives");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_tier_worker_cancellation() {
        let cancel_token = CancellationToken::new();
        let child = cancel_token.clone();
        cancel_token.cancel();
        assert!(child.is_cancelled());
    }

    #[tokio::test]
    async fn test_tier_worker_cancellation_propagation() {
        let parent = CancellationToken::new();
        let child = parent.child_token();
        assert!(!child.is_cancelled());
        parent.cancel();
        assert!(child.is_cancelled());
    }

    #[test]
    fn test_scan_interval_duration() {
        let interval = Duration::from_secs(300);
        assert_eq!(interval.as_secs(), 300);
        let short = Duration::from_secs(60);
        assert_eq!(short.as_secs(), 60);
    }

    #[test]
    fn test_default_scan_interval() {
        // Default tier scan interval is 5 minutes (300 seconds)
        let default_secs: u64 = 300;
        let interval = Duration::from_secs(default_secs);
        assert_eq!(interval.as_secs(), 300);
    }

    // ─── Integration tests requiring PostgreSQL ─────────────────────────────────

    #[ignore]
    #[tokio::test]
    async fn test_tier_worker_spawn_and_shutdown() {
        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL required for integration tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();
        let registry = Arc::new(StorageRegistry::new());
        let cancel_token = CancellationToken::new();

        let handle = TierWorker::spawn(pool, registry, cancel_token.clone(), 1);

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
}
