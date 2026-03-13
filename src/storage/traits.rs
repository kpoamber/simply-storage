use async_trait::async_trait;
use bytes::Bytes;
use std::time::Duration;

use crate::error::{AppError, AppResult};

/// Core abstraction for all storage backends.
///
/// Each backend (local disk, S3, Azure, GCS, FTP, etc.) implements this trait
/// to provide a uniform interface for file operations.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// Upload data to the given path within this storage.
    async fn upload(&self, path: &str, data: Bytes) -> AppResult<()>;

    /// Download data from the given path within this storage.
    async fn download(&self, path: &str) -> AppResult<Bytes>;

    /// Delete the file at the given path within this storage.
    async fn delete(&self, path: &str) -> AppResult<()>;

    /// Check whether a file exists at the given path.
    async fn exists(&self, path: &str) -> AppResult<bool>;

    /// Generate a temporary URL for direct access to the file.
    ///
    /// Returns `None` if the backend does not support direct URL access
    /// (e.g., FTP, SFTP, Samba). In that case, downloads are proxied through
    /// the web service.
    async fn generate_temp_url(
        &self,
        path: &str,
        expires_in: Duration,
    ) -> AppResult<Option<String>>;

    /// List files under the given prefix.
    async fn list(&self, prefix: &str) -> AppResult<Vec<String>>;

    /// List available containers/buckets on this storage backend.
    /// Returns an empty vec for backends that don't support this concept.
    async fn list_containers(&self) -> AppResult<Vec<String>> {
        Ok(vec![])
    }

    /// Create a new container/bucket on this storage backend.
    async fn create_container(&self, _name: &str) -> AppResult<()> {
        Err(AppError::BadRequest(
            "This storage backend does not support container management".to_string(),
        ))
    }

    /// Whether this backend supports container/bucket management.
    fn supports_containers(&self) -> bool {
        false
    }
}
