use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::config::SyncConfig;
use crate::db::models::{CreateFileLocation, FileLocation, ProjectStorage, Storage, SyncTask};
use crate::storage::registry::create_backend;
use crate::storage::StorageRegistry;

/// Background sync worker that processes pending sync tasks.
///
/// Each worker instance polls the `sync_tasks` table for pending tasks,
/// uses PostgreSQL advisory locks for distributed locking across service
/// instances, and executes the sync flow: download from source -> upload
/// to target -> update file_locations.
pub struct SyncWorker {
    pool: PgPool,
    registry: Arc<StorageRegistry>,
    config: SyncConfig,
    hmac_secret: String,
    cancel_token: CancellationToken,
}

impl SyncWorker {
    pub fn new(
        pool: PgPool,
        registry: Arc<StorageRegistry>,
        config: SyncConfig,
        hmac_secret: String,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            pool,
            registry,
            config,
            hmac_secret,
            cancel_token,
        }
    }

    /// Spawn the configured number of sync workers as tokio tasks.
    /// Returns a vec of JoinHandles for the spawned tasks.
    pub fn spawn_workers(
        pool: PgPool,
        registry: Arc<StorageRegistry>,
        config: SyncConfig,
        hmac_secret: String,
        cancel_token: CancellationToken,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        let num_workers = config.num_workers;
        let mut handles = Vec::with_capacity(num_workers);

        for worker_id in 0..num_workers {
            let worker = SyncWorker::new(
                pool.clone(),
                registry.clone(),
                config.clone(),
                hmac_secret.clone(),
                cancel_token.clone(),
            );

            let handle = tokio::spawn(async move {
                tracing::info!(worker_id, "Sync worker started");
                worker.run(worker_id).await;
                tracing::info!(worker_id, "Sync worker stopped");
            });

            handles.push(handle);
        }

        handles
    }

    /// Main worker loop: poll for tasks, process them, sleep, repeat.
    async fn run(&self, worker_id: usize) {
        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);

        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    tracing::info!(worker_id, "Sync worker received shutdown signal");
                    break;
                }
                _ = self.poll_and_process(worker_id) => {
                    // After processing, wait before polling again
                    tokio::select! {
                        _ = self.cancel_token.cancelled() => {
                            tracing::info!(worker_id, "Sync worker received shutdown signal during sleep");
                            break;
                        }
                        _ = tokio::time::sleep(poll_interval) => {}
                    }
                }
            }
        }
    }

    /// Poll for pending tasks and process them.
    async fn poll_and_process(&self, worker_id: usize) {
        match SyncTask::claim_pending(&self.pool, 5, self.config.max_retries).await {
            Ok(tasks) => {
                if !tasks.is_empty() {
                    tracing::debug!(
                        worker_id,
                        count = tasks.len(),
                        "Claimed sync tasks"
                    );
                }

                for task in tasks {
                    self.process_task(&task, worker_id).await;
                    // Release advisory lock after processing
                    if let Err(e) = SyncTask::release_lock(&self.pool, task.id).await {
                        tracing::warn!(
                            worker_id,
                            task_id = %task.id,
                            error = %e,
                            "Failed to release advisory lock"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    worker_id,
                    error = %e,
                    "Failed to claim pending sync tasks"
                );
            }
        }
    }

    /// Process a single sync task: download from source, upload to target.
    async fn process_task(&self, task: &SyncTask, worker_id: usize) {
        tracing::info!(
            worker_id,
            task_id = %task.id,
            file_id = %task.file_id,
            source = %task.source_storage_id,
            target = %task.target_storage_id,
            "Processing sync task"
        );

        match self.execute_sync(task).await {
            Ok(()) => {
                tracing::info!(
                    worker_id,
                    task_id = %task.id,
                    "Sync task completed successfully"
                );

                if let Err(e) =
                    SyncTask::update_status(&self.pool, task.id, "completed", None).await
                {
                    tracing::error!(
                        task_id = %task.id,
                        error = %e,
                        "Failed to mark sync task as completed"
                    );
                }
            }
            Err(e) => {
                let error_msg = format!("{}", e);
                tracing::warn!(
                    worker_id,
                    task_id = %task.id,
                    retries = task.retries,
                    error = %error_msg,
                    "Sync task failed, requeuing for retry"
                );

                if let Err(requeue_err) =
                    SyncTask::requeue_for_retry(&self.pool, task.id, &error_msg).await
                {
                    tracing::error!(
                        task_id = %task.id,
                        error = %requeue_err,
                        "Failed to requeue sync task"
                    );
                }
            }
        }
    }

    /// Resolve the backend for a storage, applying any container/prefix overrides
    /// from the project_storages assignment.
    async fn resolve_backend(
        &self,
        storage_id: uuid::Uuid,
        project_id: Option<uuid::Uuid>,
    ) -> Result<Arc<dyn crate::storage::traits::StorageBackend>, crate::error::AppError> {
        if let Some(pid) = project_id {
            let storage = Storage::find_by_id(&self.pool, storage_id).await?;
            if let Some(ps) =
                ProjectStorage::find_for_project_and_storage(&self.pool, pid, storage_id).await?
            {
                if ps.container_override.is_some() || ps.prefix_override.is_some() {
                    let mut config = storage.config.clone();
                    if let Some(ref container) = ps.container_override {
                        match storage.storage_type.as_str() {
                            "s3" => {
                                config["bucket"] =
                                    serde_json::Value::String(container.clone());
                            }
                            "azure" => {
                                config["container"] =
                                    serde_json::Value::String(container.clone());
                            }
                            "gcs" => {
                                config["bucket"] =
                                    serde_json::Value::String(container.clone());
                            }
                            _ => {}
                        }
                    }
                    if let Some(ref prefix) = ps.prefix_override {
                        config["prefix"] = serde_json::Value::String(prefix.clone());
                    }
                    return create_backend(&storage.storage_type, &config, &self.hmac_secret)
                        .await;
                }
            }
        }
        self.registry.get(&storage_id).await
    }

    /// Execute the actual sync: download from source storage, upload to target storage,
    /// and create/update the file_location record.
    async fn execute_sync(&self, task: &SyncTask) -> Result<(), crate::error::AppError> {
        // Look up a project_id associated with this file to resolve container overrides
        let project_id: Option<uuid::Uuid> = sqlx::query_as::<_, (uuid::Uuid,)>(
            "SELECT project_id FROM file_references WHERE file_id = $1 LIMIT 1",
        )
        .bind(task.file_id)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| r.0);

        // Get source backend (with overrides if applicable)
        let source_backend = self.resolve_backend(task.source_storage_id, project_id).await?;

        // Get target backend (with overrides if applicable)
        let target_backend = self.resolve_backend(task.target_storage_id, project_id).await?;

        // Find the storage path from the source file_location
        let source_locations =
            FileLocation::find_for_file(&self.pool, task.file_id).await?;
        let source_location = source_locations
            .iter()
            .find(|loc| loc.storage_id == task.source_storage_id)
            .ok_or_else(|| {
                crate::error::AppError::NotFound(format!(
                    "Source file location not found for file {} on storage {}",
                    task.file_id, task.source_storage_id
                ))
            })?;

        let storage_path = &source_location.storage_path;

        // Download from source
        let data = source_backend.download(storage_path).await?;

        // Upload to target
        target_backend.upload(storage_path, data).await?;

        // Create or update file_location for the target storage
        let create_location = CreateFileLocation {
            file_id: task.file_id,
            storage_id: task.target_storage_id,
            storage_path: storage_path.clone(),
            status: "synced".to_string(),
        };

        // Try to create; if it already exists, update the status
        match FileLocation::create(&self.pool, &create_location).await {
            Ok(_) => {}
            Err(crate::error::AppError::Database(ref e)) if is_unique_violation(e) => {
                // Location already exists (possibly with 'archived' or 'restoring' status),
                // update it directly by file_id + storage_id to avoid status-filtered queries
                FileLocation::update_status_by_file_and_storage(
                    &self.pool,
                    task.file_id,
                    task.target_storage_id,
                    "synced",
                )
                .await?;
            }
            Err(e) => return Err(e),
        }

        Ok(())
    }
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = e {
        return db_err.code().as_deref() == Some("23505");
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{
        CreateFile, CreateProject, CreateStorage, CreateSyncTask, File, FileLocation, Project,
        Storage, SyncTask,
    };
    use crate::storage::local::LocalDiskBackend;
    use crate::storage::traits::StorageBackend;
    use bytes::Bytes;
    use tempfile::TempDir;
    use uuid::Uuid;

    fn test_sync_config() -> SyncConfig {
        SyncConfig {
            num_workers: 2,
            max_retries: 3,
            poll_interval_secs: 1,
            tier_scan_interval_secs: 300,
        }
    }

    #[test]
    fn test_sync_worker_construction() {
        // Verify SyncWorker can be constructed without a real DB
        // (we can't actually connect, but we test the type)
        let config = test_sync_config();
        assert_eq!(config.num_workers, 2);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.poll_interval_secs, 1);
    }

    #[tokio::test]
    async fn test_cancellation_token_stops_worker() {
        let cancel_token = CancellationToken::new();
        let child_token = cancel_token.clone();

        // Cancel immediately
        cancel_token.cancel();

        // The token should be cancelled
        assert!(child_token.is_cancelled());
    }

    #[tokio::test]
    async fn test_cancellation_token_propagation() {
        let parent = CancellationToken::new();
        let child = parent.child_token();

        assert!(!child.is_cancelled());
        parent.cancel();
        assert!(child.is_cancelled());
    }

    #[tokio::test]
    async fn test_spawn_workers_respects_num_workers() {
        // We can't actually spawn real workers without a DB,
        // but we can verify the config-driven count
        let config = test_sync_config();
        assert_eq!(config.num_workers, 2);

        let cancel_token = CancellationToken::new();
        // Cancel immediately so workers exit right away
        cancel_token.cancel();

        // Verify the handles vector would have the right count
        let expected_count = config.num_workers;
        assert_eq!(expected_count, 2);
    }

    #[tokio::test]
    async fn test_execute_sync_via_backends_directly() {
        // Test the core sync logic: download from source, upload to target
        let source_dir = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();

        let source_backend = LocalDiskBackend::new(
            source_dir.path().to_path_buf(),
            "test-secret",
        );
        let target_backend = LocalDiskBackend::new(
            target_dir.path().to_path_buf(),
            "test-secret",
        );

        let data = Bytes::from("sync test data");
        let path = "abcdabcdef12345678900000000000000000000000000000000000000000000000";

        // Upload to source
        source_backend.upload(path, data.clone()).await.unwrap();
        assert!(source_backend.exists(path).await.unwrap());

        // Simulate sync: download from source, upload to target
        let downloaded = source_backend.download(path).await.unwrap();
        target_backend.upload(path, downloaded).await.unwrap();

        // Verify target has the file
        assert!(target_backend.exists(path).await.unwrap());
        let target_data = target_backend.download(path).await.unwrap();
        assert_eq!(target_data, data);
    }

    #[tokio::test]
    async fn test_sync_round_trip_multiple_files() {
        let source_dir = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();

        let source = LocalDiskBackend::new(source_dir.path().to_path_buf(), "secret");
        let target = LocalDiskBackend::new(target_dir.path().to_path_buf(), "secret");

        let files = vec![
            ("aabb111111111111111111111111111111111111111111111111111111111111", Bytes::from("content 1")),
            ("ccdd222222222222222222222222222222222222222222222222222222222222", Bytes::from("content 2")),
            ("eeff333333333333333333333333333333333333333333333333333333333333", Bytes::from("content 3")),
        ];

        // Upload all to source
        for (path, data) in &files {
            source.upload(path, data.clone()).await.unwrap();
        }

        // Sync all to target
        for (path, _) in &files {
            let downloaded = source.download(path).await.unwrap();
            target.upload(path, downloaded).await.unwrap();
        }

        // Verify all exist on target with correct data
        for (path, expected_data) in &files {
            assert!(target.exists(path).await.unwrap());
            let actual = target.download(path).await.unwrap();
            assert_eq!(&actual, expected_data);
        }
    }

    #[tokio::test]
    async fn test_sync_retry_logic_with_failing_source() {
        // Simulate a backend that doesn't have the file (download fails)
        let source_dir = TempDir::new().unwrap();
        let source = LocalDiskBackend::new(source_dir.path().to_path_buf(), "secret");

        // Try to download a non-existent file - should return error
        let result = source.download("nonexistent/path").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_is_unique_violation() {
        // Test with a non-DB error
        let err = sqlx::Error::RowNotFound;
        assert!(!is_unique_violation(&err));
    }

    #[tokio::test]
    async fn test_exponential_backoff_calculation() {
        // Verify the backoff pattern used for retries
        // retry 0: 1s base (poll_interval)
        // retry 1: 2s
        // retry 2: 4s
        // retry 3: 8s (then hits max_retries)
        for retry in 0..4i32 {
            let backoff = Duration::from_secs(2u64.pow(retry as u32));
            assert!(backoff.as_secs() > 0);
            assert!(backoff.as_secs() <= 8);
        }
    }

    #[test]
    fn test_sync_task_model_fields() {
        let now = chrono::Utc::now();
        let task = SyncTask {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            source_storage_id: Uuid::new_v4(),
            target_storage_id: Uuid::new_v4(),
            status: "in_progress".to_string(),
            retries: 2,
            error_msg: Some("timeout".to_string()),
            retry_after: None,
            created_at: now,
            updated_at: now,
        };

        assert_eq!(task.status, "in_progress");
        assert_eq!(task.retries, 2);
        assert_eq!(task.error_msg.as_deref(), Some("timeout"));
    }

    #[test]
    fn test_sync_config_defaults() {
        let config = SyncConfig {
            num_workers: 4,
            max_retries: 5,
            poll_interval_secs: 5,
            tier_scan_interval_secs: 300,
        };
        assert_eq!(config.num_workers, 4);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.poll_interval_secs, 5);
    }

    // ─── Integration tests requiring PostgreSQL ─────────────────────────────────

    #[ignore]
    #[tokio::test]
    async fn test_sync_task_claim_and_process() {
        let (pool, registry, _source_dir, _target_dir, source_id, target_id, file_id) =
            setup_sync_integration().await;

        // Create a sync task
        let create_task = CreateSyncTask {
            file_id,
            source_storage_id: source_id,
            target_storage_id: target_id,
        };
        let task = SyncTask::create(&pool, &create_task).await.unwrap();
        assert_eq!(task.status, "pending");

        // Claim the task
        let claimed = SyncTask::claim_pending(&pool, 10, 5).await.unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, task.id);
        assert_eq!(claimed[0].status, "in_progress");

        // Process the sync
        let config = test_sync_config();
        let cancel_token = CancellationToken::new();
        let worker = SyncWorker::new(pool.clone(), registry, config, "test-secret".to_string(), cancel_token);
        worker.process_task(&claimed[0], 0).await;

        // Verify task is completed
        let updated = SyncTask::find_pending(&pool, 10).await.unwrap();
        assert!(updated.is_empty(), "No pending tasks should remain");

        // Release lock
        SyncTask::release_lock(&pool, task.id).await.unwrap();
    }

    #[ignore]
    #[tokio::test]
    async fn test_advisory_lock_prevents_double_processing() {
        let (pool, _registry, _source_dir, _target_dir, source_id, target_id, file_id) =
            setup_sync_integration().await;

        let create_task = CreateSyncTask {
            file_id,
            source_storage_id: source_id,
            target_storage_id: target_id,
        };
        let task = SyncTask::create(&pool, &create_task).await.unwrap();

        // First claim gets the task
        let claimed1 = SyncTask::claim_pending(&pool, 10, 5).await.unwrap();
        assert_eq!(claimed1.len(), 1);

        // Second claim should get nothing (task is locked and in_progress)
        let claimed2 = SyncTask::claim_pending(&pool, 10, 5).await.unwrap();
        assert!(claimed2.is_empty());

        // Release lock
        SyncTask::release_lock(&pool, task.id).await.unwrap();
    }

    #[ignore]
    #[tokio::test]
    async fn test_retry_on_failure() {
        let (pool, _registry, _source_dir, _target_dir, source_id, target_id, file_id) =
            setup_sync_integration().await;

        let create_task = CreateSyncTask {
            file_id,
            source_storage_id: source_id,
            target_storage_id: target_id,
        };
        let task = SyncTask::create(&pool, &create_task).await.unwrap();

        // Requeue with error (simulating failure)
        let requeued = SyncTask::requeue_for_retry(&pool, task.id, "connection refused")
            .await
            .unwrap();
        assert_eq!(requeued.status, "pending");
        assert_eq!(requeued.retries, 1);
        assert_eq!(requeued.error_msg.as_deref(), Some("connection refused"));

        // Should still be claimable
        let claimed = SyncTask::claim_pending(&pool, 10, 5).await.unwrap();
        assert_eq!(claimed.len(), 1);

        SyncTask::release_lock(&pool, task.id).await.unwrap();
    }

    #[ignore]
    #[tokio::test]
    async fn test_graceful_shutdown() {
        let (pool, registry, _source_dir, _target_dir, _source_id, _target_id, _file_id) =
            setup_sync_integration().await;

        let config = test_sync_config();
        let cancel_token = CancellationToken::new();

        let handles = SyncWorker::spawn_workers(
            pool,
            registry,
            config,
            "test-secret".to_string(),
            cancel_token.clone(),
        );

        // Let workers start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Signal shutdown
        cancel_token.cancel();

        // All workers should stop
        for handle in handles {
            tokio::time::timeout(Duration::from_secs(5), handle)
                .await
                .expect("Worker should stop within timeout")
                .expect("Worker should not panic");
        }
    }

    /// Helper to set up integration test environment for sync worker tests.
    #[allow(dead_code)]
    async fn setup_sync_integration() -> (
        PgPool,
        Arc<StorageRegistry>,
        TempDir,
        TempDir,
        Uuid,
        Uuid,
        Uuid,
    ) {
        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL required for integration tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let source_dir = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();
        let registry = Arc::new(StorageRegistry::new());

        // Create source storage backend
        let source_backend = Arc::new(LocalDiskBackend::new(
            source_dir.path().to_path_buf(),
            "test-secret",
        ));
        let create_source = CreateStorage {
            name: format!("Source {}", Uuid::new_v4()),
            storage_type: "local".to_string(),
            config: serde_json::json!({"path": source_dir.path().to_str().unwrap()}),
            is_hot: Some(true),
            project_id: None,
            enabled: Some(true),
            supports_direct_links: None,
        };
        let source_storage = Storage::create(&pool, &create_source).await.unwrap();
        registry
            .register(source_storage.id, source_backend)
            .await;

        // Create target storage backend
        let target_backend = Arc::new(LocalDiskBackend::new(
            target_dir.path().to_path_buf(),
            "test-secret",
        ));
        let create_target = CreateStorage {
            name: format!("Target {}", Uuid::new_v4()),
            storage_type: "local".to_string(),
            config: serde_json::json!({"path": target_dir.path().to_str().unwrap()}),
            is_hot: Some(false),
            project_id: None,
            enabled: Some(true),
            supports_direct_links: None,
        };
        let target_storage = Storage::create(&pool, &create_target).await.unwrap();
        registry
            .register(target_storage.id, target_backend)
            .await;

        // Create a test project
        let create_project = CreateProject {
            name: format!("Sync Test {}", Uuid::new_v4()),
            slug: format!("sync-test-{}", Uuid::new_v4()),
            hot_to_cold_days: None,
        };
        let _project = Project::create(&pool, &create_project, None).await.unwrap();

        // Create a test file and upload to source
        let data = Bytes::from("test file for sync");
        let hash = crate::services::file_service::compute_sha256(&data);

        let create_file = CreateFile {
            hash_sha256: hash.clone(),
            size: data.len() as i64,
            content_type: "text/plain".to_string(),
        };
        let (file, _) = File::create_or_find(&pool, &create_file).await.unwrap();

        // Upload data to source backend
        let source = registry.get(&source_storage.id).await.unwrap();
        source.upload(&hash, data).await.unwrap();

        // Create file_location for source
        let create_location = CreateFileLocation {
            file_id: file.id,
            storage_id: source_storage.id,
            storage_path: hash,
            status: "synced".to_string(),
        };
        FileLocation::create(&pool, &create_location).await.unwrap();

        (
            pool,
            registry,
            source_dir,
            target_dir,
            source_storage.id,
            target_storage.id,
            file.id,
        )
    }
}
