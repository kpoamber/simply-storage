use chrono::{Duration as ChronoDuration, Utc};
use sqlx::PgPool;
use std::path::PathBuf;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::db::models::{FileAccessEvent, UploadSession};

/// Advisory-lock keys (classid, objid) so only one replica sweeps at a time.
const LOCK_CLASS: i32 = 0x5550; // "UP"
const LOCK_OBJ: i32 = 0x4c4e; // "LN"

/// Background worker that reclaims disk + rows from abandoned resumable uploads.
///
/// Each tick (advisory-locked to a single replica):
/// 1. expired in-progress sessions → delete temp file, mark `aborted`;
/// 2. terminal (`completed`/`aborted`) rows older than the TTL → pruned;
/// 3. orphan `*.part` files with no live in-progress session → deleted.
pub struct UploadCleanupWorker {
    pool: PgPool,
    uploads_dir: PathBuf,
    cancel_token: CancellationToken,
    interval: Duration,
    ttl: ChronoDuration,
    access_events_retention: Option<ChronoDuration>,
}

impl UploadCleanupWorker {
    pub fn new(
        pool: PgPool,
        local_temp_path: &str,
        cancel_token: CancellationToken,
        interval_secs: u64,
        ttl_secs: u64,
        access_events_retention_days: u32,
    ) -> Self {
        let access_events_retention = if access_events_retention_days == 0 {
            None
        } else {
            Some(ChronoDuration::days(access_events_retention_days as i64))
        };
        Self {
            pool,
            uploads_dir: PathBuf::from(local_temp_path).join("uploads"),
            cancel_token,
            interval: Duration::from_secs(interval_secs.max(60)),
            ttl: ChronoDuration::seconds(ttl_secs.max(60) as i64),
            access_events_retention,
        }
    }

    pub fn spawn(
        pool: PgPool,
        local_temp_path: &str,
        cancel_token: CancellationToken,
        interval_secs: u64,
        ttl_secs: u64,
        access_events_retention_days: u32,
    ) -> tokio::task::JoinHandle<()> {
        let worker = Self::new(
            pool,
            local_temp_path,
            cancel_token,
            interval_secs,
            ttl_secs,
            access_events_retention_days,
        );
        tokio::spawn(async move {
            tracing::info!("Upload cleanup worker started");
            worker.run().await;
            tracing::info!("Upload cleanup worker stopped");
        })
    }

    async fn run(&self) {
        // Reconcile orphans once at startup before entering the periodic loop.
        self.sweep().await;
        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => break,
                _ = tokio::time::sleep(self.interval) => self.sweep().await,
            }
        }
    }

    async fn sweep(&self) {
        // Single-replica guard.
        let locked: (bool,) = match sqlx::query_as("SELECT pg_try_advisory_lock($1, $2)")
            .bind(LOCK_CLASS)
            .bind(LOCK_OBJ)
            .fetch_one(&self.pool)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "Upload cleanup: failed to take advisory lock");
                return;
            }
        };
        if !locked.0 {
            return; // another replica is sweeping
        }

        self.sweep_expired().await;
        self.prune_terminal().await;
        self.reconcile_orphans().await;
        self.prune_access_events().await;

        let _ = sqlx::query("SELECT pg_advisory_unlock($1, $2)")
            .bind(LOCK_CLASS)
            .bind(LOCK_OBJ)
            .execute(&self.pool)
            .await;
    }

    async fn sweep_expired(&self) {
        let expired = match UploadSession::find_expired(&self.pool, 500).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(error = %e, "Upload cleanup: failed to list expired sessions");
                return;
            }
        };
        for s in expired {
            let _ = tokio::fs::remove_file(&s.temp_path).await;
            if let Err(e) = UploadSession::mark_status(&self.pool, s.id, "aborted").await {
                tracing::warn!(upload_id = %s.id, error = %e, "Upload cleanup: mark aborted failed");
            }
        }
    }

    async fn prune_terminal(&self) {
        let cutoff = Utc::now() - self.ttl;
        match UploadSession::prune_terminal(&self.pool, cutoff).await {
            Ok(n) if n > 0 => tracing::info!(pruned = n, "Upload cleanup: pruned terminal sessions"),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "Upload cleanup: prune failed"),
        }
    }

    /// Drop file_access_events rows older than the configured retention.
    async fn prune_access_events(&self) {
        let Some(retention) = self.access_events_retention else { return; };
        let cutoff = Utc::now() - retention;
        match FileAccessEvent::prune_older_than(&self.pool, cutoff).await {
            Ok(n) if n > 0 => tracing::info!(pruned = n, "Pruned old file_access_events"),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "Failed to prune file_access_events"),
        }
    }

    /// Delete `*.part` files that have no matching in-progress session (crash debris).
    async fn reconcile_orphans(&self) {
        let mut entries = match tokio::fs::read_dir(&self.uploads_dir).await {
            Ok(e) => e,
            Err(_) => return, // dir may not exist yet
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            if path.extension().and_then(|e| e.to_str()) != Some("part") {
                continue;
            }
            let id = match Uuid::parse_str(stem) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let live = matches!(
                UploadSession::find_by_id(&self.pool, id).await,
                Ok(s) if s.status == "in_progress"
            );
            if !live {
                let _ = tokio::fs::remove_file(&path).await;
            }
        }
    }
}
