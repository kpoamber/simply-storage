use async_trait::async_trait;
use bytes::Bytes;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::storage::traits::StorageBackend;

/// Configuration for Samba/SMB storage backend.
#[derive(Debug, Clone)]
pub struct SambaConfig {
    /// SMB server hostname or IP.
    pub host: String,
    /// SMB share name.
    pub share: String,
    /// SMB username.
    pub username: String,
    /// SMB password.
    pub password: String,
    /// Optional workgroup/domain.
    pub workgroup: String,
    /// Base path within the share for storing files.
    pub base_path: String,
}

/// Samba/SMB storage backend using the pavao crate.
///
/// Since pavao provides a synchronous API, all I/O operations are run inside
/// `tokio::task::spawn_blocking` to avoid blocking the async runtime.
///
/// Returns `None` from `generate_temp_url()` — temp access is proxied through
/// the web service.
pub struct SambaBackend {
    config: SambaConfig,
}

impl SambaBackend {
    pub fn new(config: SambaConfig) -> Self {
        Self { config }
    }

    /// Build the SMB URI for the share root.
    fn smb_uri(&self) -> String {
        format!("smb://{}/{}", self.config.host, self.config.share)
    }

    /// Build the full remote path from a logical storage path.
    fn remote_path(&self, path: &str) -> String {
        if self.config.base_path.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", self.config.base_path.trim_end_matches('/'), path)
        }
    }

    /// Build the full SMB URL for a file path.
    fn file_url(&self, path: &str) -> String {
        let remote = self.remote_path(path);
        format!("{}/{}", self.smb_uri(), remote)
    }

    /// Connect to the SMB server via pavao.
    fn connect_sync(&self) -> AppResult<pavao::SmbClient> {
        let client = pavao::SmbClient::new(
            pavao::SmbCredentials::default()
                .server(&format!("smb://{}", self.config.host))
                .share(&self.config.share)
                .username(&self.config.username)
                .password(&self.config.password)
                .workgroup(&self.config.workgroup),
        )
        .map_err(|e| AppError::Internal(format!("SMB connect failed: {}", e)))?;

        Ok(client)
    }

    /// Ensure the parent directory of a remote path exists, creating it recursively.
    fn ensure_parent_dirs_sync(
        &self,
        client: &pavao::SmbClient,
        remote_path: &str,
    ) -> AppResult<()> {
        if let Some((parent, _)) = remote_path.rsplit_once('/') {
            if !parent.is_empty() {
                let parts: Vec<&str> = parent.split('/').filter(|p| !p.is_empty()).collect();
                let mut current = String::new();
                for part in parts {
                    if current.is_empty() {
                        current = part.to_string();
                    } else {
                        current = format!("{}/{}", current, part);
                    }
                    let dir_url = format!("{}/{}", self.smb_uri(), current);
                    // Try to create directory; ignore errors if it already exists
                    let _ = client.mkdir(&dir_url, 0o755);
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl StorageBackend for SambaBackend {
    async fn upload(&self, path: &str, data: Bytes) -> AppResult<()> {
        let file_url = self.file_url(path);
        let smb_uri = self.smb_uri();
        let remote = self.remote_path(path);
        let config = self.config.clone();

        tokio::task::spawn_blocking(move || {
            let client = pavao::SmbClient::new(
                pavao::SmbCredentials::default()
                    .server(&format!("smb://{}", config.host))
                    .share(&config.share)
                    .username(&config.username)
                    .password(&config.password)
                    .workgroup(&config.workgroup),
            )
            .map_err(|e| AppError::Internal(format!("SMB connect failed: {}", e)))?;

            // Ensure parent dirs
            if let Some((parent, _)) = remote.rsplit_once('/') {
                if !parent.is_empty() {
                    let parts: Vec<&str> = parent.split('/').filter(|p| !p.is_empty()).collect();
                    let mut current = String::new();
                    for part in parts {
                        if current.is_empty() {
                            current = part.to_string();
                        } else {
                            current = format!("{}/{}", current, part);
                        }
                        let dir_url = format!("{}/{}", smb_uri, current);
                        let _ = client.mkdir(&dir_url, 0o755);
                    }
                }
            }

            // Open file for writing
            let fd = client
                .open_with(&file_url, pavao::SmbOpenOptions::default().create(true).write(true))
                .map_err(|e| AppError::Internal(format!("SMB open for write failed: {}", e)))?;

            client
                .write(fd, &data)
                .map_err(|e| AppError::Internal(format!("SMB write failed: {}", e)))?;

            client
                .close(fd)
                .map_err(|e| AppError::Internal(format!("SMB close failed: {}", e)))?;

            Ok(())
        })
        .await
        .map_err(|e| AppError::Internal(format!("SMB upload task failed: {}", e)))?
    }

    async fn download(&self, path: &str) -> AppResult<Bytes> {
        let file_url = self.file_url(path);
        let config = self.config.clone();

        tokio::task::spawn_blocking(move || {
            let client = pavao::SmbClient::new(
                pavao::SmbCredentials::default()
                    .server(&format!("smb://{}", config.host))
                    .share(&config.share)
                    .username(&config.username)
                    .password(&config.password)
                    .workgroup(&config.workgroup),
            )
            .map_err(|e| AppError::Internal(format!("SMB connect failed: {}", e)))?;

            let fd = client
                .open_with(&file_url, pavao::SmbOpenOptions::default().read(true))
                .map_err(|e| {
                    let msg = format!("{}", e);
                    if msg.contains("No such file") || msg.contains("not found") {
                        AppError::NotFound(format!("File not found on SMB: {}", path))
                    } else {
                        AppError::Internal(format!("SMB open for read failed: {}", e))
                    }
                })?;

            let data = client
                .read(fd)
                .map_err(|e| AppError::Internal(format!("SMB read failed: {}", e)))?;

            client.close(fd).ok();

            Ok(Bytes::from(data))
        })
        .await
        .map_err(|e| AppError::Internal(format!("SMB download task failed: {}", e)))?
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let file_url = self.file_url(path);
        let config = self.config.clone();

        tokio::task::spawn_blocking(move || {
            let client = pavao::SmbClient::new(
                pavao::SmbCredentials::default()
                    .server(&format!("smb://{}", config.host))
                    .share(&config.share)
                    .username(&config.username)
                    .password(&config.password)
                    .workgroup(&config.workgroup),
            )
            .map_err(|e| AppError::Internal(format!("SMB connect failed: {}", e)))?;

            match client.unlink(&file_url) {
                Ok(_) => Ok(()),
                Err(e) => {
                    let msg = format!("{}", e);
                    if msg.contains("No such file") || msg.contains("not found") {
                        Ok(()) // Idempotent delete
                    } else {
                        Err(AppError::Internal(format!("SMB delete failed: {}", e)))
                    }
                }
            }
        })
        .await
        .map_err(|e| AppError::Internal(format!("SMB delete task failed: {}", e)))?
    }

    async fn exists(&self, path: &str) -> AppResult<bool> {
        let file_url = self.file_url(path);
        let config = self.config.clone();

        tokio::task::spawn_blocking(move || {
            let client = pavao::SmbClient::new(
                pavao::SmbCredentials::default()
                    .server(&format!("smb://{}", config.host))
                    .share(&config.share)
                    .username(&config.username)
                    .password(&config.password)
                    .workgroup(&config.workgroup),
            )
            .map_err(|e| AppError::Internal(format!("SMB connect failed: {}", e)))?;

            match client.stat(&file_url) {
                Ok(_) => Ok(true),
                Err(_) => Ok(false),
            }
        })
        .await
        .map_err(|e| AppError::Internal(format!("SMB exists task failed: {}", e)))?
    }

    async fn generate_temp_url(
        &self,
        _path: &str,
        _expires_in: Duration,
        _filename: Option<&str>,
    ) -> AppResult<Option<String>> {
        // SMB does not support direct URL access; downloads are proxied through the web service.
        Ok(None)
    }

    async fn list(&self, prefix: &str) -> AppResult<Vec<String>> {
        let dir_url = if self.config.base_path.is_empty() {
            self.smb_uri()
        } else {
            format!("{}/{}", self.smb_uri(), self.config.base_path)
        };
        let prefix = prefix.to_string();
        let config = self.config.clone();

        tokio::task::spawn_blocking(move || {
            let client = pavao::SmbClient::new(
                pavao::SmbCredentials::default()
                    .server(&format!("smb://{}", config.host))
                    .share(&config.share)
                    .username(&config.username)
                    .password(&config.password)
                    .workgroup(&config.workgroup),
            )
            .map_err(|e| AppError::Internal(format!("SMB connect failed: {}", e)))?;

            let entries = client
                .readdir(&dir_url)
                .map_err(|e| AppError::Internal(format!("SMB list failed: {}", e)))?;

            let results: Vec<String> = entries
                .into_iter()
                .filter_map(|entry| {
                    let name = entry.name().to_string();
                    if name == "." || name == ".." {
                        return None;
                    }
                    if prefix.is_empty() || name.starts_with(&prefix) {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect();

            Ok(results)
        })
        .await
        .map_err(|e| AppError::Internal(format!("SMB list task failed: {}", e)))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SambaConfig {
        SambaConfig {
            host: "127.0.0.1".to_string(),
            share: "testshare".to_string(),
            username: "test".to_string(),
            password: "test".to_string(),
            workgroup: "WORKGROUP".to_string(),
            base_path: "".to_string(),
        }
    }

    fn test_config_with_base_path() -> SambaConfig {
        SambaConfig {
            base_path: "storage/files".to_string(),
            ..test_config()
        }
    }

    #[test]
    fn test_remote_path_no_base() {
        let backend = SambaBackend::new(test_config());
        assert_eq!(backend.remote_path("abcdef123"), "abcdef123");
    }

    #[test]
    fn test_remote_path_with_base() {
        let backend = SambaBackend::new(test_config_with_base_path());
        assert_eq!(
            backend.remote_path("abcdef123"),
            "storage/files/abcdef123"
        );
    }

    #[test]
    fn test_remote_path_trailing_slash() {
        let config = SambaConfig {
            base_path: "storage/files/".to_string(),
            ..test_config()
        };
        let backend = SambaBackend::new(config);
        assert_eq!(backend.remote_path("test.txt"), "storage/files/test.txt");
    }

    #[test]
    fn test_smb_uri() {
        let backend = SambaBackend::new(test_config());
        assert_eq!(backend.smb_uri(), "smb://127.0.0.1/testshare");
    }

    #[test]
    fn test_file_url() {
        let backend = SambaBackend::new(test_config_with_base_path());
        assert_eq!(
            backend.file_url("abcdef123"),
            "smb://127.0.0.1/testshare/storage/files/abcdef123"
        );
    }

    #[tokio::test]
    async fn test_generate_temp_url_returns_none() {
        let backend = SambaBackend::new(test_config());
        let result = backend
            .generate_temp_url("test.txt", Duration::from_secs(3600), None)
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_config_clone() {
        let config = test_config();
        let cloned = config.clone();
        assert_eq!(cloned.host, "127.0.0.1");
        assert_eq!(cloned.share, "testshare");
        assert_eq!(cloned.workgroup, "WORKGROUP");
    }

    #[test]
    fn test_config_debug() {
        let config = test_config();
        let debug = format!("{:?}", config);
        assert!(debug.contains("127.0.0.1"));
        assert!(debug.contains("testshare"));
    }
}
