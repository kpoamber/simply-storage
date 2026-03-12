use async_trait::async_trait;
use bytes::Bytes;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{AppError, AppResult};
use crate::storage::traits::StorageBackend;

type HmacSha256 = Hmac<Sha256>;

/// Local disk storage backend using content-addressable storage.
///
/// Files are stored using a hash-based directory structure: the first two
/// characters of the storage path form the top-level directory, the next two
/// form a subdirectory, and the full path is the filename. For example, a path
/// of `abcdef0123...` is stored at `<base>/ab/cd/abcdef0123...`.
pub struct LocalDiskBackend {
    base_path: PathBuf,
    hmac_secret: String,
}

impl LocalDiskBackend {
    pub fn new(base_path: impl Into<PathBuf>, hmac_secret: impl Into<String>) -> Self {
        Self {
            base_path: base_path.into(),
            hmac_secret: hmac_secret.into(),
        }
    }

    /// Convert a logical storage path into the on-disk path with hash-based directory structure.
    fn resolve_path(&self, path: &str) -> PathBuf {
        if path.len() >= 4 {
            let dir1 = &path[..2];
            let dir2 = &path[2..4];
            self.base_path.join(dir1).join(dir2).join(path)
        } else {
            self.base_path.join(path)
        }
    }

    /// Generate an HMAC-SHA256 signature for a temp URL token.
    fn sign_token(&self, path: &str, expires_at: u64) -> String {
        let message = format!("{}:{}", path, expires_at);
        let mut mac =
            HmacSha256::new_from_slice(self.hmac_secret.as_bytes()).expect("HMAC accepts any key");
        mac.update(message.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Verify an HMAC-signed temp URL token.
    ///
    /// Returns `true` if the signature is valid and the token has not expired.
    pub fn verify_temp_url(
        &self,
        path: &str,
        expires_at: u64,
        signature: &str,
    ) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now > expires_at {
            return false;
        }
        let expected = self.sign_token(path, expires_at);
        crate::constant_time_eq(expected.as_bytes(), signature.as_bytes())
    }
}

#[async_trait]
impl StorageBackend for LocalDiskBackend {
    async fn upload(&self, path: &str, data: Bytes) -> AppResult<()> {
        let file_path = self.resolve_path(path);
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&file_path, &data).await?;
        Ok(())
    }

    async fn download(&self, path: &str) -> AppResult<Bytes> {
        let file_path = self.resolve_path(path);
        if !file_path.exists() {
            return Err(AppError::NotFound(format!(
                "File not found in local storage: {}",
                path
            )));
        }
        let data = tokio::fs::read(&file_path).await?;
        Ok(Bytes::from(data))
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let file_path = self.resolve_path(path);
        if file_path.exists() {
            tokio::fs::remove_file(&file_path).await?;
            // Clean up empty parent directories (best-effort)
            cleanup_empty_parents(&file_path, &self.base_path).await;
        }
        Ok(())
    }

    async fn exists(&self, path: &str) -> AppResult<bool> {
        let file_path = self.resolve_path(path);
        Ok(file_path.exists())
    }

    async fn generate_temp_url(
        &self,
        path: &str,
        expires_in: Duration,
    ) -> AppResult<Option<String>> {
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + expires_in.as_secs();

        let signature = self.sign_token(path, expires_at);
        let encoded_path = urlencoding::encode(path);
        let url = format!(
            "/download/local?path={}&expires={}&sig={}",
            encoded_path, expires_at, signature
        );
        Ok(Some(url))
    }

    async fn list(&self, prefix: &str) -> AppResult<Vec<String>> {
        let search_dir = if prefix.is_empty() {
            self.base_path.clone()
        } else if prefix.len() >= 4 {
            let dir1 = &prefix[..2];
            let dir2 = &prefix[2..4];
            self.base_path.join(dir1).join(dir2)
        } else if prefix.len() >= 2 {
            self.base_path.join(&prefix[..2])
        } else {
            self.base_path.clone()
        };

        if !search_dir.exists() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        collect_files(&search_dir, &self.base_path, prefix, &mut results).await?;
        Ok(results)
    }
}

/// Recursively collect files under a directory, filtering by prefix.
async fn collect_files(
    dir: &Path,
    base: &Path,
    prefix: &str,
    results: &mut Vec<String>,
) -> AppResult<()> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            Box::pin(collect_files(&path, base, prefix, results)).await?;
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if prefix.is_empty() || name.starts_with(prefix) {
                results.push(name.to_string());
            }
        }
    }
    Ok(())
}

/// Remove empty parent directories up to (but not including) the base path.
async fn cleanup_empty_parents(file_path: &Path, base_path: &Path) {
    let mut current = file_path.parent();
    while let Some(dir) = current {
        if dir == base_path {
            break;
        }
        // Try to remove; if it's not empty this will fail silently.
        if tokio::fs::remove_dir(dir).await.is_err() {
            break;
        }
        current = dir.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, LocalDiskBackend) {
        let dir = TempDir::new().unwrap();
        let backend = LocalDiskBackend::new(dir.path().to_path_buf(), "test-secret");
        (dir, backend)
    }

    #[tokio::test]
    async fn test_upload_download_roundtrip() {
        let (_dir, backend) = setup();
        let data = Bytes::from("hello world");
        let path = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

        backend.upload(path, data.clone()).await.unwrap();
        let downloaded = backend.download(path).await.unwrap();
        assert_eq!(downloaded, data);
    }

    #[tokio::test]
    async fn test_content_addressable_directory_structure() {
        let (dir, backend) = setup();
        let data = Bytes::from("test data");
        let path = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

        backend.upload(path, data).await.unwrap();

        // Verify the hash-based directory structure
        let expected = dir.path().join("ab").join("cd").join(path);
        assert!(expected.exists());
    }

    #[tokio::test]
    async fn test_exists() {
        let (_dir, backend) = setup();
        let path = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

        assert!(!backend.exists(path).await.unwrap());
        backend.upload(path, Bytes::from("data")).await.unwrap();
        assert!(backend.exists(path).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete() {
        let (_dir, backend) = setup();
        let path = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

        backend.upload(path, Bytes::from("data")).await.unwrap();
        assert!(backend.exists(path).await.unwrap());

        backend.delete(path).await.unwrap();
        assert!(!backend.exists(path).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_is_ok() {
        let (_dir, backend) = setup();
        // Deleting a file that doesn't exist should not error
        backend.delete("nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn test_download_nonexistent_returns_not_found() {
        let (_dir, backend) = setup();
        let result = backend.download("nonexistent").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::NotFound(_) => {}
            other => panic!("Expected NotFound, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_list_files() {
        let (_dir, backend) = setup();
        let hash1 = "aabb1111111111111111111111111111111111111111111111111111111111111";
        let hash2 = "aabb2222222222222222222222222222222222222222222222222222222222222";
        let hash3 = "ccdd3333333333333333333333333333333333333333333333333333333333333";

        backend.upload(hash1, Bytes::from("one")).await.unwrap();
        backend.upload(hash2, Bytes::from("two")).await.unwrap();
        backend.upload(hash3, Bytes::from("three")).await.unwrap();

        // List all
        let mut all = backend.list("").await.unwrap();
        all.sort();
        assert_eq!(all.len(), 3);

        // List with prefix
        let filtered = backend.list("aabb").await.unwrap();
        assert_eq!(filtered.len(), 2);
    }

    #[tokio::test]
    async fn test_list_empty_prefix_no_files() {
        let (_dir, backend) = setup();
        let result = backend.list("").await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_temp_url_generation() {
        let (_dir, backend) = setup();
        let path = "abcdef0123456789";
        let url = backend
            .generate_temp_url(path, Duration::from_secs(3600))
            .await
            .unwrap();

        assert!(url.is_some());
        let url = url.unwrap();
        assert!(url.starts_with("/download/local?path="));
        assert!(url.contains("expires="));
        assert!(url.contains("sig="));
    }

    #[tokio::test]
    async fn test_temp_url_verification_valid() {
        let (_dir, backend) = setup();
        let path = "abcdef0123456789";
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;

        let signature = backend.sign_token(path, expires_at);
        assert!(backend.verify_temp_url(path, expires_at, &signature));
    }

    #[tokio::test]
    async fn test_temp_url_verification_expired() {
        let (_dir, backend) = setup();
        let path = "abcdef0123456789";
        // Set expiry in the past
        let expires_at = 1000;
        let signature = backend.sign_token(path, expires_at);
        assert!(!backend.verify_temp_url(path, expires_at, &signature));
    }

    #[tokio::test]
    async fn test_temp_url_verification_wrong_signature() {
        let (_dir, backend) = setup();
        let path = "abcdef0123456789";
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;

        assert!(!backend.verify_temp_url(path, expires_at, "bad-signature"));
    }

    #[tokio::test]
    async fn test_temp_url_verification_wrong_path() {
        let (_dir, backend) = setup();
        let path = "abcdef0123456789";
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;

        let signature = backend.sign_token(path, expires_at);
        // Verify with a different path should fail
        assert!(!backend.verify_temp_url("different-path", expires_at, &signature));
    }

    #[test]
    fn test_constant_time_eq() {
        use crate::constant_time_eq;
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
    }

    #[test]
    fn test_resolve_path_hash_structure() {
        let backend = LocalDiskBackend::new("/data/storage", "secret");
        let path = "abcdef0123456789";
        let resolved = backend.resolve_path(path);
        assert_eq!(
            resolved,
            PathBuf::from("/data/storage/ab/cd/abcdef0123456789")
        );
    }

    #[test]
    fn test_resolve_path_short() {
        let backend = LocalDiskBackend::new("/data/storage", "secret");
        let path = "ab";
        let resolved = backend.resolve_path(path);
        assert_eq!(resolved, PathBuf::from("/data/storage/ab"));
    }
}
