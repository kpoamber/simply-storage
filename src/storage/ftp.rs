use async_trait::async_trait;
use bytes::Bytes;
use std::io::Cursor;
use std::time::Duration;
use tokio::io::AsyncReadExt;

use crate::error::{AppError, AppResult};
use crate::storage::traits::StorageBackend;

/// Type alias for the async FTP stream (tokio, non-TLS).
type AsyncFtpStream = suppaftp::tokio::AsyncFtpStream;

/// Configuration for FTP storage backend.
#[derive(Debug, Clone)]
pub struct FtpConfig {
    /// FTP server hostname or IP.
    pub host: String,
    /// FTP server port (default: 21).
    pub port: u16,
    /// FTP username.
    pub username: String,
    /// FTP password.
    pub password: String,
    /// Base path on the FTP server for storing files.
    pub base_path: String,
}

/// FTP storage backend using the suppaftp crate in async mode (tokio).
///
/// Each operation creates a new FTP connection for simplicity and reliability.
/// Returns `None` from `generate_temp_url()` — temp access is proxied through
/// the web service.
pub struct FtpBackend {
    config: FtpConfig,
}

impl FtpBackend {
    pub fn new(config: FtpConfig) -> Self {
        Self { config }
    }

    /// Build the full remote path from a logical storage path.
    fn remote_path(&self, path: &str) -> String {
        if self.config.base_path.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", self.config.base_path.trim_end_matches('/'), path)
        }
    }

    /// Connect to the FTP server and authenticate.
    async fn connect(&self) -> AppResult<AsyncFtpStream> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let mut ftp = AsyncFtpStream::connect(&addr)
            .await
            .map_err(|e| AppError::Internal(format!("FTP connect failed: {}", e)))?;

        ftp.login(&self.config.username, &self.config.password)
            .await
            .map_err(|e| AppError::Internal(format!("FTP login failed: {}", e)))?;

        ftp.transfer_type(suppaftp::types::FileType::Binary)
            .await
            .map_err(|e| AppError::Internal(format!("FTP set binary mode failed: {}", e)))?;

        Ok(ftp)
    }

    /// Ensure the parent directory of a remote path exists, creating it recursively.
    async fn ensure_parent_dirs(
        &self,
        ftp: &mut AsyncFtpStream,
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
                    // Try to create directory; ignore errors if it already exists
                    let _ = ftp.mkdir(&current).await;
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl StorageBackend for FtpBackend {
    async fn upload(&self, path: &str, data: Bytes) -> AppResult<()> {
        let remote = self.remote_path(path);
        let mut ftp = self.connect().await?;

        self.ensure_parent_dirs(&mut ftp, &remote).await?;

        let mut cursor = Cursor::new(data.to_vec());
        ftp.put_file(&remote, &mut cursor)
            .await
            .map_err(|e| AppError::Internal(format!("FTP upload failed: {}", e)))?;

        let _ = ftp.quit().await;
        Ok(())
    }

    async fn download(&self, path: &str) -> AppResult<Bytes> {
        let remote = self.remote_path(path);
        let mut ftp = self.connect().await?;

        let mut stream = ftp.retr_as_stream(&remote).await.map_err(|e| {
            let msg = format!("{}", e);
            if msg.contains("550") {
                AppError::NotFound(format!("File not found on FTP: {}", path))
            } else {
                AppError::Internal(format!("FTP download failed: {}", e))
            }
        })?;

        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .await
            .map_err(|e| AppError::Internal(format!("FTP read data failed: {}", e)))?;

        ftp.finalize_retr_stream(stream)
            .await
            .map_err(|e| AppError::Internal(format!("FTP finalize stream failed: {}", e)))?;

        let _ = ftp.quit().await;
        Ok(Bytes::from(buf))
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let remote = self.remote_path(path);
        let mut ftp = self.connect().await?;

        match ftp.rm(&remote).await {
            Ok(_) => {}
            Err(e) => {
                let msg = format!("{}", e);
                // 550 = file not found; treat as success (idempotent delete)
                if !msg.contains("550") {
                    let _ = ftp.quit().await;
                    return Err(AppError::Internal(format!("FTP delete failed: {}", e)));
                }
            }
        }

        let _ = ftp.quit().await;
        Ok(())
    }

    async fn exists(&self, path: &str) -> AppResult<bool> {
        let remote = self.remote_path(path);
        let mut ftp = self.connect().await?;

        let exists = ftp.size(&remote).await.is_ok();

        let _ = ftp.quit().await;
        Ok(exists)
    }

    async fn generate_temp_url(
        &self,
        _path: &str,
        _expires_in: Duration,
    ) -> AppResult<Option<String>> {
        // FTP does not support direct URL access; downloads are proxied through the web service.
        Ok(None)
    }

    async fn list(&self, prefix: &str) -> AppResult<Vec<String>> {
        let dir = if self.config.base_path.is_empty() {
            ".".to_string()
        } else {
            self.config.base_path.clone()
        };

        let mut ftp = self.connect().await?;

        let entries = ftp
            .nlst(Some(&dir))
            .await
            .map_err(|e| AppError::Internal(format!("FTP list failed: {}", e)))?;

        let _ = ftp.quit().await;

        let base = if self.config.base_path.is_empty() {
            String::new()
        } else {
            format!("{}/", self.config.base_path.trim_end_matches('/'))
        };

        Ok(entries
            .into_iter()
            .map(|e| {
                e.strip_prefix(&base)
                    .unwrap_or(&e)
                    .to_string()
            })
            .filter(|name| prefix.is_empty() || name.starts_with(prefix))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> FtpConfig {
        FtpConfig {
            host: "127.0.0.1".to_string(),
            port: 2121,
            username: "test".to_string(),
            password: "test".to_string(),
            base_path: "".to_string(),
        }
    }

    fn test_config_with_base_path() -> FtpConfig {
        FtpConfig {
            base_path: "storage/files".to_string(),
            ..test_config()
        }
    }

    #[test]
    fn test_remote_path_no_base() {
        let backend = FtpBackend::new(test_config());
        assert_eq!(backend.remote_path("abcdef123"), "abcdef123");
        assert_eq!(backend.remote_path("path/to/file"), "path/to/file");
    }

    #[test]
    fn test_remote_path_with_base() {
        let backend = FtpBackend::new(test_config_with_base_path());
        assert_eq!(
            backend.remote_path("abcdef123"),
            "storage/files/abcdef123"
        );
    }

    #[test]
    fn test_remote_path_trailing_slash() {
        let config = FtpConfig {
            base_path: "storage/files/".to_string(),
            ..test_config()
        };
        let backend = FtpBackend::new(config);
        assert_eq!(backend.remote_path("test.txt"), "storage/files/test.txt");
    }

    #[tokio::test]
    async fn test_generate_temp_url_returns_none() {
        let backend = FtpBackend::new(test_config());
        let result = backend
            .generate_temp_url("test.txt", Duration::from_secs(3600))
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_config_clone() {
        let config = test_config();
        let cloned = config.clone();
        assert_eq!(cloned.host, "127.0.0.1");
        assert_eq!(cloned.port, 2121);
        assert_eq!(cloned.username, "test");
    }

    #[test]
    fn test_config_debug() {
        let config = test_config();
        let debug = format!("{:?}", config);
        assert!(debug.contains("127.0.0.1"));
        assert!(debug.contains("2121"));
    }

    #[tokio::test]
    async fn test_upload_to_nonexistent_host_returns_error() {
        let config = FtpConfig {
            port: 19990,
            ..test_config()
        };
        let backend = FtpBackend::new(config);

        let result = backend.upload("test.txt", Bytes::from("hello")).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::Internal(msg) => assert!(msg.contains("FTP")),
            other => panic!("Expected Internal error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_download_from_nonexistent_host_returns_error() {
        let config = FtpConfig {
            port: 19990,
            ..test_config()
        };
        let backend = FtpBackend::new(config);

        let result = backend.download("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_from_nonexistent_host_returns_error() {
        let config = FtpConfig {
            port: 19990,
            ..test_config()
        };
        let backend = FtpBackend::new(config);

        let result = backend.delete("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_exists_on_nonexistent_host_returns_error() {
        let config = FtpConfig {
            port: 19990,
            ..test_config()
        };
        let backend = FtpBackend::new(config);

        let result = backend.exists("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_on_nonexistent_host_returns_error() {
        let config = FtpConfig {
            port: 19990,
            ..test_config()
        };
        let backend = FtpBackend::new(config);

        let result = backend.list("").await;
        assert!(result.is_err());
    }
}
