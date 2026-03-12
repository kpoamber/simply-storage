use async_trait::async_trait;
use bytes::Bytes;
use russh::client;
use russh_sftp::client::SftpSession;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};
use crate::storage::traits::StorageBackend;

/// Configuration for SFTP storage backend.
#[derive(Debug, Clone)]
pub struct SftpConfig {
    /// SSH server hostname or IP.
    pub host: String,
    /// SSH server port (default: 22).
    pub port: u16,
    /// SSH username.
    pub username: String,
    /// SSH password.
    pub password: String,
    /// Base path on the remote server for storing files.
    pub base_path: String,
}

/// Minimal SSH client handler that accepts all server keys.
struct SshHandler;

impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        // Accept all server keys. In production, verify against known_hosts.
        Ok(true)
    }
}

/// SFTP storage backend using russh + russh-sftp.
///
/// Each operation creates a new SSH/SFTP connection for simplicity and reliability.
/// Returns `None` from `generate_temp_url()` — temp access is proxied through
/// the web service.
pub struct SftpBackend {
    config: SftpConfig,
}

impl SftpBackend {
    pub fn new(config: SftpConfig) -> Self {
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

    /// Establish an SSH connection and open an SFTP session.
    async fn connect(&self) -> AppResult<SftpSession> {
        let ssh_config = Arc::new(client::Config::default());
        let addr = format!("{}:{}", self.config.host, self.config.port);

        let mut handle = client::connect(ssh_config, &*addr, SshHandler)
            .await
            .map_err(|e| AppError::Internal(format!("SSH connect failed: {}", e)))?;

        let auth_result = handle
            .authenticate_password(&self.config.username, &self.config.password)
            .await
            .map_err(|e| AppError::Internal(format!("SSH auth failed: {}", e)))?;

        if !auth_result.success() {
            return Err(AppError::Internal(
                "SSH authentication failed: invalid credentials".to_string(),
            ));
        }

        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| AppError::Internal(format!("SSH channel open failed: {}", e)))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| AppError::Internal(format!("SFTP subsystem request failed: {}", e)))?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| AppError::Internal(format!("SFTP session init failed: {}", e)))?;

        Ok(sftp)
    }

    /// Ensure the parent directory of a remote path exists, creating it recursively.
    async fn ensure_parent_dirs(&self, sftp: &SftpSession, remote_path: &str) -> AppResult<()> {
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
                    let _ = sftp.create_dir(&current).await;
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl StorageBackend for SftpBackend {
    async fn upload(&self, path: &str, data: Bytes) -> AppResult<()> {
        let remote = self.remote_path(path);
        let sftp = self.connect().await?;

        self.ensure_parent_dirs(&sftp, &remote).await?;

        // Use create() which opens with CREATE | TRUNCATE | WRITE flags
        let mut file = sftp
            .create(&remote)
            .await
            .map_err(|e| AppError::Internal(format!("SFTP create file failed: {}", e)))?;

        file.write_all(&data)
            .await
            .map_err(|e| AppError::Internal(format!("SFTP write failed: {}", e)))?;

        file.shutdown()
            .await
            .map_err(|e| AppError::Internal(format!("SFTP close file failed: {}", e)))?;

        Ok(())
    }

    async fn download(&self, path: &str) -> AppResult<Bytes> {
        let remote = self.remote_path(path);
        let sftp = self.connect().await?;

        let data = sftp.read(&remote).await.map_err(|e| {
            let msg = format!("{}", e);
            if msg.contains("No such file") || msg.contains("no such file") {
                AppError::NotFound(format!("File not found on SFTP: {}", path))
            } else {
                AppError::Internal(format!("SFTP download failed: {}", e))
            }
        })?;

        Ok(Bytes::from(data))
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let remote = self.remote_path(path);
        let sftp = self.connect().await?;

        match sftp.remove_file(&remote).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("{}", e);
                // Treat "not found" as success (idempotent delete)
                if msg.contains("No such file") || msg.contains("no such file") {
                    Ok(())
                } else {
                    Err(AppError::Internal(format!("SFTP delete failed: {}", e)))
                }
            }
        }
    }

    async fn exists(&self, path: &str) -> AppResult<bool> {
        let remote = self.remote_path(path);
        let sftp = self.connect().await?;

        match sftp.try_exists(&remote).await {
            Ok(exists) => Ok(exists),
            Err(e) => Err(AppError::Internal(format!(
                "SFTP exists check failed: {}",
                e
            ))),
        }
    }

    async fn generate_temp_url(
        &self,
        _path: &str,
        _expires_in: Duration,
    ) -> AppResult<Option<String>> {
        // SFTP does not support direct URL access; downloads are proxied through the web service.
        Ok(None)
    }

    async fn list(&self, prefix: &str) -> AppResult<Vec<String>> {
        let dir = if self.config.base_path.is_empty() {
            ".".to_string()
        } else {
            self.config.base_path.clone()
        };

        let sftp = self.connect().await?;

        let entries = sftp
            .read_dir(&dir)
            .await
            .map_err(|e| AppError::Internal(format!("SFTP list failed: {}", e)))?;

        // ReadDir already filters "." and ".."
        let results: Vec<String> = entries
            .filter_map(|entry| {
                let name = entry.file_name();
                if prefix.is_empty() || name.starts_with(prefix) {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SftpConfig {
        SftpConfig {
            host: "127.0.0.1".to_string(),
            port: 2222,
            username: "test".to_string(),
            password: "test".to_string(),
            base_path: "".to_string(),
        }
    }

    fn test_config_with_base_path() -> SftpConfig {
        SftpConfig {
            base_path: "storage/files".to_string(),
            ..test_config()
        }
    }

    #[test]
    fn test_remote_path_no_base() {
        let backend = SftpBackend::new(test_config());
        assert_eq!(backend.remote_path("abcdef123"), "abcdef123");
        assert_eq!(backend.remote_path("path/to/file"), "path/to/file");
    }

    #[test]
    fn test_remote_path_with_base() {
        let backend = SftpBackend::new(test_config_with_base_path());
        assert_eq!(
            backend.remote_path("abcdef123"),
            "storage/files/abcdef123"
        );
    }

    #[test]
    fn test_remote_path_trailing_slash() {
        let config = SftpConfig {
            base_path: "storage/files/".to_string(),
            ..test_config()
        };
        let backend = SftpBackend::new(config);
        assert_eq!(backend.remote_path("test.txt"), "storage/files/test.txt");
    }

    #[tokio::test]
    async fn test_generate_temp_url_returns_none() {
        let backend = SftpBackend::new(test_config());
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
        assert_eq!(cloned.port, 2222);
        assert_eq!(cloned.username, "test");
    }

    #[test]
    fn test_config_debug() {
        let config = test_config();
        let debug = format!("{:?}", config);
        assert!(debug.contains("127.0.0.1"));
        assert!(debug.contains("2222"));
    }

    #[tokio::test]
    async fn test_upload_to_nonexistent_host_returns_error() {
        let config = SftpConfig {
            port: 19991,
            ..test_config()
        };
        let backend = SftpBackend::new(config);

        let result = backend.upload("test.txt", Bytes::from("hello")).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::Internal(msg) => assert!(msg.contains("SSH")),
            other => panic!("Expected Internal error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_download_from_nonexistent_host_returns_error() {
        let config = SftpConfig {
            port: 19991,
            ..test_config()
        };
        let backend = SftpBackend::new(config);

        let result = backend.download("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_from_nonexistent_host_returns_error() {
        let config = SftpConfig {
            port: 19991,
            ..test_config()
        };
        let backend = SftpBackend::new(config);

        let result = backend.delete("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_exists_on_nonexistent_host_returns_error() {
        let config = SftpConfig {
            port: 19991,
            ..test_config()
        };
        let backend = SftpBackend::new(config);

        let result = backend.exists("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_on_nonexistent_host_returns_error() {
        let config = SftpConfig {
            port: 19991,
            ..test_config()
        };
        let backend = SftpBackend::new(config);

        let result = backend.list("").await;
        assert!(result.is_err());
    }
}
