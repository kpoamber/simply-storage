use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::storage::traits::StorageBackend;
use crate::storage::{
    AzureBlobBackend, AzureBlobConfig, FtpBackend, FtpConfig, GcsBackend, GcsConfig,
    HetznerStorageBoxBackend, HetznerStorageBoxConfig, LocalDiskBackend, S3Config,
    S3StorageBackend, SftpBackend, SftpConfig,
};

/// Thread-safe registry for looking up storage backends by their storage ID.
///
/// Supports dynamic registration and removal of backends at runtime,
/// allowing hot-reload of storage configurations.
pub struct StorageRegistry {
    backends: RwLock<HashMap<Uuid, Arc<dyn StorageBackend>>>,
}

impl StorageRegistry {
    pub fn new() -> Self {
        Self {
            backends: RwLock::new(HashMap::new()),
        }
    }

    /// Register a storage backend under the given ID.
    /// Replaces any existing backend with the same ID.
    pub async fn register(&self, id: Uuid, backend: Arc<dyn StorageBackend>) {
        let mut backends = self.backends.write().await;
        backends.insert(id, backend);
    }

    /// Remove a storage backend by ID.
    /// Returns the removed backend if it existed.
    pub async fn unregister(&self, id: &Uuid) -> Option<Arc<dyn StorageBackend>> {
        let mut backends = self.backends.write().await;
        backends.remove(id)
    }

    /// Look up a storage backend by ID.
    pub async fn get(&self, id: &Uuid) -> AppResult<Arc<dyn StorageBackend>> {
        let backends = self.backends.read().await;
        backends
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("Storage backend not found: {}", id)))
    }

    /// Check whether a backend with the given ID is registered.
    pub async fn contains(&self, id: &Uuid) -> bool {
        let backends = self.backends.read().await;
        backends.contains_key(id)
    }

    /// Return the number of registered backends.
    pub async fn len(&self) -> usize {
        let backends = self.backends.read().await;
        backends.len()
    }

    /// Return whether the registry is empty.
    pub async fn is_empty(&self) -> bool {
        let backends = self.backends.read().await;
        backends.is_empty()
    }

    /// Return all registered storage IDs.
    pub async fn list_ids(&self) -> Vec<Uuid> {
        let backends = self.backends.read().await;
        backends.keys().copied().collect()
    }
}

impl Default for StorageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a storage backend instance from its type name and JSON config.
pub async fn create_backend(
    storage_type: &str,
    config: &serde_json::Value,
    hmac_secret: &str,
) -> AppResult<Arc<dyn StorageBackend>> {
    match storage_type {
        "local" => {
            let path = config["path"]
                .as_str()
                .ok_or_else(|| AppError::BadRequest("local storage requires 'path' config".to_string()))?;
            Ok(Arc::new(LocalDiskBackend::new(path, hmac_secret)))
        }
        "s3" => {
            let s3_config = S3Config {
                endpoint_url: config["endpoint_url"].as_str().map(|s| s.to_string()),
                region: config["region"].as_str().unwrap_or("us-east-1").to_string(),
                bucket: config["bucket"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("s3 storage requires 'bucket' config".to_string()))?
                    .to_string(),
                prefix: config["prefix"].as_str().unwrap_or("").to_string(),
                access_key_id: config["access_key_id"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("s3 storage requires 'access_key_id'".to_string()))?
                    .to_string(),
                secret_access_key: config["secret_access_key"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("s3 storage requires 'secret_access_key'".to_string()))?
                    .to_string(),
                multipart_threshold: config["multipart_threshold"].as_u64(),
                part_size: config["part_size"].as_u64(),
                force_path_style: config["force_path_style"].as_bool().unwrap_or(false),
            };
            Ok(Arc::new(S3StorageBackend::new(s3_config).await))
        }
        "azure" => {
            let azure_config = AzureBlobConfig {
                account_name: config["account_name"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("azure storage requires 'account_name'".to_string()))?
                    .to_string(),
                account_key: config["account_key"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("azure storage requires 'account_key'".to_string()))?
                    .to_string(),
                container: config["container"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("azure storage requires 'container'".to_string()))?
                    .to_string(),
                prefix: config["prefix"].as_str().unwrap_or("").to_string(),
                endpoint: config["endpoint"].as_str().map(|s| s.to_string()),
            };
            Ok(Arc::new(AzureBlobBackend::new(azure_config)?))
        }
        "gcs" => {
            let gcs_config = GcsConfig {
                bucket: config["bucket"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("gcs storage requires 'bucket'".to_string()))?
                    .to_string(),
                prefix: config["prefix"].as_str().unwrap_or("").to_string(),
                client_email: config["client_email"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("gcs storage requires 'client_email'".to_string()))?
                    .to_string(),
                private_key_pem: config["private_key_pem"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("gcs storage requires 'private_key_pem'".to_string()))?
                    .to_string(),
                token_uri: config["token_uri"].as_str().map(|s| s.to_string()),
            };
            Ok(Arc::new(GcsBackend::new(gcs_config)?))
        }
        "hetzner" => {
            let hetzner_config = HetznerStorageBoxConfig {
                host: config["host"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("hetzner storage requires 'host'".to_string()))?
                    .to_string(),
                username: config["username"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("hetzner storage requires 'username'".to_string()))?
                    .to_string(),
                password: config["password"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("hetzner storage requires 'password'".to_string()))?
                    .to_string(),
                port: config["port"].as_u64().unwrap_or(443) as u16,
                base_path: config["base_path"].as_str().unwrap_or("").to_string(),
                sub_account: config["sub_account"].as_str().map(|s| s.to_string()),
            };
            Ok(Arc::new(HetznerStorageBoxBackend::new(hetzner_config)))
        }
        "ftp" => {
            let ftp_config = FtpConfig {
                host: config["host"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("ftp storage requires 'host'".to_string()))?
                    .to_string(),
                port: config["port"].as_u64().unwrap_or(21) as u16,
                username: config["username"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("ftp storage requires 'username'".to_string()))?
                    .to_string(),
                password: config["password"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("ftp storage requires 'password'".to_string()))?
                    .to_string(),
                base_path: config["base_path"].as_str().unwrap_or("").to_string(),
            };
            Ok(Arc::new(FtpBackend::new(ftp_config)))
        }
        "sftp" => {
            let sftp_config = SftpConfig {
                host: config["host"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("sftp storage requires 'host'".to_string()))?
                    .to_string(),
                port: config["port"].as_u64().unwrap_or(22) as u16,
                username: config["username"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("sftp storage requires 'username'".to_string()))?
                    .to_string(),
                password: config["password"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("sftp storage requires 'password'".to_string()))?
                    .to_string(),
                base_path: config["base_path"].as_str().unwrap_or("").to_string(),
            };
            Ok(Arc::new(SftpBackend::new(sftp_config)))
        }
        #[cfg(feature = "samba")]
        "samba" => {
            let samba_config = crate::storage::samba::SambaConfig {
                host: config["host"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("samba storage requires 'host'".to_string()))?
                    .to_string(),
                share: config["share"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("samba storage requires 'share'".to_string()))?
                    .to_string(),
                username: config["username"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("samba storage requires 'username'".to_string()))?
                    .to_string(),
                password: config["password"]
                    .as_str()
                    .ok_or_else(|| AppError::BadRequest("samba storage requires 'password'".to_string()))?
                    .to_string(),
                workgroup: config["workgroup"].as_str().unwrap_or("WORKGROUP").to_string(),
                base_path: config["base_path"].as_str().unwrap_or("").to_string(),
            };
            Ok(Arc::new(crate::storage::samba::SambaBackend::new(samba_config)))
        }
        #[cfg(not(feature = "samba"))]
        "samba" => Err(AppError::BadRequest(
            "Samba storage requires the 'samba' feature to be enabled at compile time".to_string(),
        )),
        other => Err(AppError::BadRequest(format!(
            "Unsupported storage type: {}",
            other
        ))),
    }
}

/// Load all enabled storages from the database and register their backends.
pub async fn load_backends_from_db(
    pool: &sqlx::PgPool,
    registry: &StorageRegistry,
    hmac_secret: &str,
) -> AppResult<()> {
    let storages = crate::db::models::Storage::list_enabled(pool).await?;

    for storage in storages {
        match create_backend(&storage.storage_type, &storage.config, hmac_secret).await {
            Ok(backend) => {
                registry.register(storage.id, backend).await;
                tracing::info!(
                    storage_id = %storage.id,
                    name = %storage.name,
                    storage_type = %storage.storage_type,
                    "Registered storage backend"
                );
            }
            Err(e) => {
                tracing::warn!(
                    storage_id = %storage.id,
                    name = %storage.name,
                    storage_type = %storage.storage_type,
                    error = %e,
                    "Failed to initialize storage backend, skipping"
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::local::LocalDiskBackend;

    fn make_backend(path: &str) -> Arc<dyn StorageBackend> {
        Arc::new(LocalDiskBackend::new(path, "test-secret"))
    }

    #[tokio::test]
    async fn test_register_and_get() {
        let registry = StorageRegistry::new();
        let id = Uuid::new_v4();
        let backend = make_backend("/tmp/test1");

        registry.register(id, backend).await;
        assert!(registry.get(&id).await.is_ok());
    }

    #[tokio::test]
    async fn test_get_nonexistent_returns_not_found() {
        let registry = StorageRegistry::new();
        let id = Uuid::new_v4();

        let result = registry.get(&id).await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_unregister() {
        let registry = StorageRegistry::new();
        let id = Uuid::new_v4();
        let backend = make_backend("/tmp/test2");

        registry.register(id, backend).await;
        assert!(registry.contains(&id).await);

        let removed = registry.unregister(&id).await;
        assert!(removed.is_some());
        assert!(!registry.contains(&id).await);
    }

    #[tokio::test]
    async fn test_unregister_nonexistent_returns_none() {
        let registry = StorageRegistry::new();
        let id = Uuid::new_v4();
        assert!(registry.unregister(&id).await.is_none());
    }

    #[tokio::test]
    async fn test_replace_existing() {
        let registry = StorageRegistry::new();
        let id = Uuid::new_v4();

        registry.register(id, make_backend("/tmp/old")).await;
        registry.register(id, make_backend("/tmp/new")).await;

        // Should still have exactly one entry
        assert_eq!(registry.len().await, 1);
        assert!(registry.get(&id).await.is_ok());
    }

    #[tokio::test]
    async fn test_len_and_is_empty() {
        let registry = StorageRegistry::new();
        assert!(registry.is_empty().await);
        assert_eq!(registry.len().await, 0);

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        registry.register(id1, make_backend("/tmp/a")).await;
        registry.register(id2, make_backend("/tmp/b")).await;

        assert!(!registry.is_empty().await);
        assert_eq!(registry.len().await, 2);
    }

    #[tokio::test]
    async fn test_list_ids() {
        let registry = StorageRegistry::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        registry.register(id1, make_backend("/tmp/a")).await;
        registry.register(id2, make_backend("/tmp/b")).await;

        let mut ids = registry.list_ids().await;
        ids.sort();
        let mut expected = vec![id1, id2];
        expected.sort();
        assert_eq!(ids, expected);
    }
}
