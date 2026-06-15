use async_trait::async_trait;
use bytes::Bytes;
use std::path::Path;
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

    /// Upload the contents of a local file to the given path within this storage.
    ///
    /// Used by resumable/chunked uploads, where the assembled payload lives in a
    /// temp file and may be too large to hold in memory. The default reads the
    /// whole file into memory and delegates to [`upload`] — backends where memory
    /// matters (local disk rename, S3 multipart) override this with a streaming
    /// implementation. `_size` is the known file length, a hint for multipart sizing.
    async fn upload_from_file(&self, path: &str, src: &Path, _size: u64) -> AppResult<()> {
        let data = tokio::fs::read(src)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to read temp file: {}", e)))?;
        self.upload(path, Bytes::from(data)).await
    }

    /// Download data from the given path within this storage.
    async fn download(&self, path: &str) -> AppResult<Bytes>;

    /// Stream the contents of the remote object to a local file.
    ///
    /// Used by background sync / large-file proxy paths where loading the whole
    /// payload into memory (`download`) would OOM the process. The default
    /// falls back to `download` + write — backends where memory matters (local
    /// disk copy, Hetzner WebDAV streaming, S3 streaming) override this with
    /// a chunked implementation.
    async fn download_to_file(&self, path: &str, dst: &Path) -> AppResult<()> {
        let data = self.download(path).await?;
        tokio::fs::write(dst, &data)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to write download to {:?}: {}", dst, e)))
    }

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
        filename: Option<&str>,
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
