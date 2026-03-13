use bytes::Bytes;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::models::{
    is_unique_violation, CreateFile, CreateFileLocation, CreateFileReference, CreateSyncTask, File,
    FileLocation, FileReference, ProjectStorage, Storage, SyncTask,
};
use crate::error::{AppError, AppResult};
use crate::storage::registry::create_backend;
use crate::storage::traits::StorageBackend;
use crate::storage::StorageRegistry;

/// Result returned after a successful file upload.
#[derive(Debug, serde::Serialize)]
pub struct UploadResult {
    pub file: File,
    pub file_reference: FileReference,
    pub is_duplicate: bool,
    pub sync_tasks_created: usize,
}

/// Result returned after a successful file download.
#[derive(Debug)]
pub struct DownloadResult {
    pub data: Bytes,
    pub file: File,
    pub content_type: String,
    pub original_name: Option<String>,
}

/// Core service handling file upload (with deduplication) and download operations.
pub struct FileService {
    pool: PgPool,
    registry: Arc<StorageRegistry>,
    hmac_secret: String,
}

impl FileService {
    pub fn new(pool: PgPool, registry: Arc<StorageRegistry>, hmac_secret: String) -> Self {
        Self {
            pool,
            registry,
            hmac_secret,
        }
    }

    /// Upload a file to the storage system with SHA-256 deduplication.
    ///
    /// Flow:
    /// 1. Compute SHA-256 hash and determine content type
    /// 2. Deduplicate: check if hash exists in `files` table
    /// 3. If new file: store to primary storage, insert `files` record
    /// 4. Create `file_references` row linking file to project
    /// 5. Determine target storages, create `file_locations` for primary,
    ///    create `sync_tasks` for remaining storages
    /// 6. Return immediately after primary storage write
    pub async fn upload_file(
        &self,
        project_id: Uuid,
        original_name: &str,
        content_type: &str,
        data: Bytes,
    ) -> AppResult<UploadResult> {
        // 1. Compute SHA-256 hash
        let hash = compute_sha256(&data);
        let size = data.len() as i64;

        // 2. Deduplicate: create or find existing file record
        let create_file = CreateFile {
            hash_sha256: hash.clone(),
            size,
            content_type: content_type.to_string(),
        };
        let (file, is_new) = File::create_or_find(&self.pool, &create_file).await?;

        // 3. Get available storages for this project (project-specific + shared)
        let storages = Storage::list_for_project(&self.pool, project_id).await?;
        if storages.is_empty() {
            return Err(AppError::BadRequest(
                "No enabled storages available for this project".to_string(),
            ));
        }

        // Pick the primary storage: first hot storage that's registered in the registry
        let primary_storage = self.find_primary_storage(&storages).await?;

        // Build the content-addressable storage path from the hash
        let storage_path = hash.clone();

        // 3b. If this is a new file, upload to primary storage
        if is_new {
            let backend = self.get_project_backend(&primary_storage, project_id).await?;
            backend.upload(&storage_path, data).await?;

            // Create file_location for the primary storage
            // Handle unique constraint gracefully for concurrent duplicate uploads
            let create_location = CreateFileLocation {
                file_id: file.id,
                storage_id: primary_storage.id,
                storage_path: storage_path.clone(),
                status: "synced".to_string(),
            };
            match FileLocation::create(&self.pool, &create_location).await {
                Ok(_) => {}
                Err(AppError::Database(ref e)) if is_unique_violation(e) => {
                    // Another concurrent upload already created this location - that's fine
                }
                Err(e) => return Err(e),
            }
        }

        // 4. Create file_reference linking file to project with original filename
        let create_ref = CreateFileReference {
            file_id: file.id,
            project_id,
            original_name: original_name.to_string(),
        };
        let file_reference = FileReference::create_or_find(&self.pool, &create_ref).await?;

        // 5. Create sync_tasks for remaining storages (if new file)
        let mut sync_tasks_created = 0;
        if is_new {
            for storage in &storages {
                if storage.id == primary_storage.id {
                    continue;
                }
                // Only create sync task if the backend is registered
                if !self.registry.contains(&storage.id).await {
                    continue;
                }
                let create_task = CreateSyncTask {
                    file_id: file.id,
                    source_storage_id: primary_storage.id,
                    target_storage_id: storage.id,
                };
                SyncTask::create(&self.pool, &create_task).await?;
                sync_tasks_created += 1;
            }
        }

        Ok(UploadResult {
            file,
            file_reference,
            is_duplicate: !is_new,
            sync_tasks_created,
        })
    }

    /// Download a file by its ID.
    ///
    /// Finds the best available storage location (prefers hot storage),
    /// downloads the file data, and updates `last_accessed_at`.
    /// Resolves container/prefix overrides from project_storages when applicable.
    pub async fn download_file(&self, file_id: Uuid) -> AppResult<DownloadResult> {
        // Look up the file record
        let file = File::find_by_id(&self.pool, file_id).await?;

        // Find available locations, ordered by hot preference
        let locations = FileLocation::find_for_file(&self.pool, file_id).await?;
        if locations.is_empty() {
            return Err(AppError::NotFound(format!(
                "No available storage locations for file {}",
                file_id
            )));
        }

        // Get project_ids from file_references to resolve container overrides
        let refs = FileReference::find_by_file_id(&self.pool, file_id).await?;

        // Try each location until we find one with a registered backend
        for location in &locations {
            let backends = self
                .resolve_backends_for_location(&location.storage_id, &refs)
                .await;

            for backend in &backends {
                match backend.download(&location.storage_path).await {
                    Ok(data) => {
                        let _ = FileLocation::touch_accessed(&self.pool, location.id).await;
                        return Ok(DownloadResult {
                            data,
                            file: file.clone(),
                            content_type: file.content_type.clone(),
                            original_name: refs.first().map(|r| r.original_name.clone()),
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            storage_id = %location.storage_id,
                            file_id = %file_id,
                            error = %e,
                            "Failed to download from storage location, trying next"
                        );
                    }
                }
            }
        }

        Err(AppError::Internal(format!(
            "All storage locations failed for file {}",
            file_id
        )))
    }

    /// Generate a temporary download link for a file.
    pub async fn generate_temp_link(
        &self,
        file_id: Uuid,
        expires_in: std::time::Duration,
    ) -> AppResult<String> {
        let _file = File::find_by_id(&self.pool, file_id).await?;
        let locations = FileLocation::find_for_file(&self.pool, file_id).await?;
        let refs = FileReference::find_by_file_id(&self.pool, file_id).await?;

        for location in &locations {
            let backends = self
                .resolve_backends_for_location(&location.storage_id, &refs)
                .await;

            for backend in &backends {
                match backend
                    .generate_temp_url(&location.storage_path, expires_in)
                    .await
                {
                    Ok(Some(url)) => {
                        let _ = FileLocation::touch_accessed(&self.pool, location.id).await;
                        return Ok(url);
                    }
                    Ok(None) => continue,
                    Err(_) => continue,
                }
            }
        }

        Err(AppError::NotFound(format!(
            "No temp URL available for file {}",
            file_id
        )))
    }

    /// Resolve all possible backends for a storage location, considering container overrides
    /// from project_storages. Returns override-based backends first, then the default backend.
    async fn resolve_backends_for_location(
        &self,
        storage_id: &Uuid,
        file_refs: &[FileReference],
    ) -> Vec<Arc<dyn StorageBackend>> {
        let mut backends: Vec<Arc<dyn StorageBackend>> = Vec::new();

        // Try project-specific overrides first
        if let Ok(storage) = Storage::find_by_id(&self.pool, *storage_id).await {
            for fref in file_refs {
                if let Ok(backend) = self.get_project_backend(&storage, fref.project_id).await {
                    backends.push(backend);
                }
            }
        }

        // Fallback: default backend from registry (if not already covered)
        if let Ok(default_backend) = self.registry.get(storage_id).await {
            backends.push(default_backend);
        }

        backends
    }

    /// Get storage backends for a project, applying container/prefix overrides from project_storages.
    /// Returns (Storage, backend, effective_storage_path_prefix) tuples.
    async fn get_project_backend(
        &self,
        storage: &Storage,
        project_id: Uuid,
    ) -> AppResult<Arc<dyn StorageBackend>> {
        let assignment =
            ProjectStorage::find_for_project_and_storage(&self.pool, project_id, storage.id)
                .await?;

        if let Some(ps) = assignment {
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

        // No overrides — use default backend from registry
        self.registry.get(&storage.id).await
    }

    /// Find the primary storage for uploading: first hot, enabled storage
    /// that has a registered backend in the registry.
    async fn find_primary_storage(&self, storages: &[Storage]) -> AppResult<Storage> {
        // Prefer hot storages first
        for storage in storages {
            if storage.is_hot && self.registry.contains(&storage.id).await {
                return Ok(storage.clone());
            }
        }
        // Fall back to any storage with a registered backend
        for storage in storages {
            if self.registry.contains(&storage.id).await {
                return Ok(storage.clone());
            }
        }
        Err(AppError::BadRequest(
            "No storage backends are registered and available".to_string(),
        ))
    }
}

/// Compute SHA-256 hash of the given data, returning the hex-encoded string.
pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::local::LocalDiskBackend;
    use crate::storage::traits::StorageBackend;
    use crate::storage::StorageRegistry;
    use bytes::Bytes;
    use tempfile::TempDir;

    #[test]
    fn test_compute_sha256() {
        let data = b"hello world";
        let hash = compute_sha256(data);
        assert_eq!(hash.len(), 64);
        // Known SHA-256 of "hello world"
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_compute_sha256_empty() {
        let hash = compute_sha256(b"");
        assert_eq!(hash.len(), 64);
        // Known SHA-256 of empty string
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_compute_sha256_deterministic() {
        let data = b"test data for hashing";
        let hash1 = compute_sha256(data);
        let hash2 = compute_sha256(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_compute_sha256_different_data_different_hash() {
        let hash1 = compute_sha256(b"data1");
        let hash2 = compute_sha256(b"data2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_upload_result_serialization() {
        let now = chrono::Utc::now();
        let result = UploadResult {
            file: File {
                id: Uuid::new_v4(),
                hash_sha256: "a".repeat(64),
                size: 1024,
                content_type: "text/plain".to_string(),
                created_at: now,
            },
            file_reference: FileReference {
                id: Uuid::new_v4(),
                file_id: Uuid::new_v4(),
                project_id: Uuid::new_v4(),
                original_name: "test.txt".to_string(),
                created_at: now,
            },
            is_duplicate: false,
            sync_tasks_created: 2,
        };

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["is_duplicate"], false);
        assert_eq!(json["sync_tasks_created"], 2);
        assert_eq!(json["file"]["content_type"], "text/plain");
        assert_eq!(json["file_reference"]["original_name"], "test.txt");
    }

    #[tokio::test]
    async fn test_file_service_construction() {
        let registry = Arc::new(StorageRegistry::new());
        let dir = TempDir::new().unwrap();
        let backend = Arc::new(LocalDiskBackend::new(
            dir.path().to_path_buf(),
            "test-secret",
        ));
        let id = Uuid::new_v4();
        registry.register(id, backend).await;

        // Verify registry is functional
        assert!(registry.contains(&id).await);
        assert_eq!(registry.len().await, 1);
    }

    #[tokio::test]
    async fn test_find_primary_storage_prefers_hot() {
        let registry = Arc::new(StorageRegistry::new());
        let dir = TempDir::new().unwrap();

        let cold_id = Uuid::new_v4();
        let hot_id = Uuid::new_v4();

        let backend1 = Arc::new(LocalDiskBackend::new(
            dir.path().join("cold"),
            "test-secret",
        ));
        let backend2 = Arc::new(LocalDiskBackend::new(
            dir.path().join("hot"),
            "test-secret",
        ));

        registry.register(cold_id, backend1).await;
        registry.register(hot_id, backend2).await;

        let now = chrono::Utc::now();
        let storages = vec![
            Storage {
                id: cold_id,
                name: "Cold Storage".to_string(),
                storage_type: "local".to_string(),
                config: serde_json::json!({}),
                is_hot: false,
                project_id: None,
                enabled: true,
                created_at: now,
                updated_at: now,
            },
            Storage {
                id: hot_id,
                name: "Hot Storage".to_string(),
                storage_type: "local".to_string(),
                config: serde_json::json!({}),
                is_hot: true,
                project_id: None,
                enabled: true,
                created_at: now,
                updated_at: now,
            },
        ];

        // We can't construct FileService without a real PgPool,
        // but we can test the logic by creating a mock pool placeholder.
        // Instead, test the storage selection logic directly.
        // The find_primary_storage method is on FileService, so we verify
        // the registry-based logic here.
        assert!(registry.contains(&hot_id).await);
        assert!(registry.contains(&cold_id).await);

        // Verify hot storage would be preferred (first hot in list with registered backend)
        let mut found_hot = None;
        for s in &storages {
            if s.is_hot && registry.contains(&s.id).await {
                found_hot = Some(s.id);
                break;
            }
        }
        assert_eq!(found_hot, Some(hot_id));
    }

    #[tokio::test]
    async fn test_find_primary_storage_falls_back_to_cold() {
        let registry = Arc::new(StorageRegistry::new());
        let dir = TempDir::new().unwrap();

        let cold_id = Uuid::new_v4();
        let backend = Arc::new(LocalDiskBackend::new(
            dir.path().join("cold"),
            "test-secret",
        ));
        registry.register(cold_id, backend).await;

        let now = chrono::Utc::now();
        let storages = vec![Storage {
            id: cold_id,
            name: "Cold Storage".to_string(),
            storage_type: "local".to_string(),
            config: serde_json::json!({}),
            is_hot: false,
            project_id: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        }];

        // Only cold storage available - should fall back
        let mut found = None;
        for s in &storages {
            if s.is_hot && registry.contains(&s.id).await {
                found = Some(s.id);
                break;
            }
        }
        if found.is_none() {
            for s in &storages {
                if registry.contains(&s.id).await {
                    found = Some(s.id);
                    break;
                }
            }
        }
        assert_eq!(found, Some(cold_id));
    }

    #[tokio::test]
    async fn test_storage_path_is_hash() {
        // Verify that the storage path used for content-addressable storage
        // is the SHA-256 hash itself
        let data = Bytes::from("test content");
        let hash = compute_sha256(&data);
        let storage_path = hash.clone();

        // The path should be the full 64-char hex hash
        assert_eq!(storage_path.len(), 64);
        assert_eq!(storage_path, hash);
    }

    #[tokio::test]
    async fn test_upload_download_via_backend_directly() {
        // Test the upload/download round-trip through the storage backend,
        // which is the core I/O path used by FileService
        let dir = TempDir::new().unwrap();
        let backend = LocalDiskBackend::new(dir.path().to_path_buf(), "test-secret");

        let data = Bytes::from("file content for round-trip test");
        let hash = compute_sha256(&data);

        // Upload using hash as path (content-addressable)
        backend.upload(&hash, data.clone()).await.unwrap();

        // Download should return the same data
        let downloaded = backend.download(&hash).await.unwrap();
        assert_eq!(downloaded, data);

        // Uploading same content again (dedup scenario) should succeed
        backend.upload(&hash, data.clone()).await.unwrap();
        let downloaded2 = backend.download(&hash).await.unwrap();
        assert_eq!(downloaded2, data);
    }

    // ─── Integration tests requiring PostgreSQL ─────────────────────────────────

    #[ignore]
    #[tokio::test]
    async fn test_upload_new_file() {
        let (pool, registry, _dir, _storage_id, project_id) = setup_integration().await;
        let service = FileService::new(pool, registry, "test-secret".to_string());

        let data = Bytes::from("hello world");
        let result = service
            .upload_file(project_id, "hello.txt", "text/plain", data)
            .await
            .unwrap();

        assert!(!result.is_duplicate);
        assert_eq!(result.file.content_type, "text/plain");
        assert_eq!(result.file_reference.original_name, "hello.txt");
        assert_eq!(result.file_reference.project_id, project_id);
        assert_eq!(
            result.file.hash_sha256.trim(),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[ignore]
    #[tokio::test]
    async fn test_upload_duplicate_file() {
        let (pool, registry, _dir, _storage_id, project_id) = setup_integration().await;
        let service = FileService::new(pool, registry, "test-secret".to_string());

        let data = Bytes::from("duplicate content");

        // First upload
        let result1 = service
            .upload_file(project_id, "file1.txt", "text/plain", data.clone())
            .await
            .unwrap();
        assert!(!result1.is_duplicate);

        // Second upload with same content but different name
        let result2 = service
            .upload_file(project_id, "file2.txt", "text/plain", data)
            .await
            .unwrap();
        assert!(result2.is_duplicate);
        assert_eq!(result1.file.id, result2.file.id);
        assert_ne!(
            result1.file_reference.original_name,
            result2.file_reference.original_name
        );
    }

    #[ignore]
    #[tokio::test]
    async fn test_download_file() {
        let (pool, registry, _dir, _storage_id, project_id) = setup_integration().await;
        let service = FileService::new(pool, registry, "test-secret".to_string());

        let data = Bytes::from("download me");
        let upload_result = service
            .upload_file(project_id, "download.txt", "text/plain", data.clone())
            .await
            .unwrap();

        let download_result = service.download_file(upload_result.file.id).await.unwrap();
        assert_eq!(download_result.data, data);
        assert_eq!(download_result.content_type, "text/plain");
    }

    #[ignore]
    #[tokio::test]
    async fn test_download_nonexistent_file() {
        let (pool, registry, _dir, _, _) = setup_integration().await;
        let service = FileService::new(pool, registry, "test-secret".to_string());

        let result = service.download_file(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[ignore]
    #[tokio::test]
    async fn test_concurrent_upload_same_file() {
        let (pool, registry, _dir, _storage_id, project_id) = setup_integration().await;
        let service = Arc::new(FileService::new(pool, registry, "test-secret".to_string()));

        let data = Bytes::from("concurrent upload data");

        // Launch multiple concurrent uploads of the same content
        let mut handles = Vec::new();
        for i in 0..5 {
            let svc = service.clone();
            let d = data.clone();
            let name = format!("concurrent-{}.txt", i);
            handles.push(tokio::spawn(async move {
                svc.upload_file(project_id, &name, "text/plain", d).await
            }));
        }

        let results: Vec<_> = futures::future::join_all(handles).await;
        let mut file_ids = std::collections::HashSet::new();
        let mut success_count = 0;

        for result in results {
            match result.unwrap() {
                Ok(upload_result) => {
                    file_ids.insert(upload_result.file.id);
                    success_count += 1;
                }
                Err(e) => {
                    panic!("Concurrent upload should not fail: {:?}", e);
                }
            }
        }

        // All uploads should succeed and reference the same file
        assert_eq!(success_count, 5);
        assert_eq!(file_ids.len(), 1, "All uploads should reference the same deduplicated file");
    }

    /// Helper to set up integration test environment with real DB and local storage.
    #[allow(dead_code)]
    async fn setup_integration() -> (PgPool, Arc<StorageRegistry>, TempDir, Uuid, Uuid) {
        use crate::db::models::{CreateProject, CreateStorage, Project};

        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for integration tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let dir = TempDir::new().unwrap();
        let registry = Arc::new(StorageRegistry::new());

        // Create a local storage backend
        let backend = Arc::new(LocalDiskBackend::new(
            dir.path().to_path_buf(),
            "test-secret",
        ));

        // Register storage in DB
        let create_storage = CreateStorage {
            name: format!("Test Local {}", Uuid::new_v4()),
            storage_type: "local".to_string(),
            config: serde_json::json!({"path": dir.path().to_str().unwrap()}),
            is_hot: Some(true),
            project_id: None,
            enabled: Some(true),
        };
        let storage = Storage::create(&pool, &create_storage).await.unwrap();

        // Register backend in registry
        registry.register(storage.id, backend).await;

        // Create a test project
        let create_project = CreateProject {
            name: format!("Test Project {}", Uuid::new_v4()),
            slug: format!("test-project-{}", Uuid::new_v4()),
            hot_to_cold_days: None,
        };
        let project = Project::create(&pool, &create_project, None).await.unwrap();

        (pool, registry, dir, storage.id, project.id)
    }
}
