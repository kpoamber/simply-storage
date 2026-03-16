use async_trait::async_trait;
use bytes::Bytes;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::storage::traits::StorageBackend;

/// Configuration for Hetzner StorageBox backend (WebDAV).
#[derive(Debug, Clone)]
pub struct HetznerStorageBoxConfig {
    /// StorageBox hostname (e.g. `uXXXXXX.your-storagebox.de`).
    pub host: String,
    /// WebDAV port (default: 443).
    pub port: u16,
    /// Username (main account or sub-account).
    pub username: String,
    /// Password.
    pub password: String,
    /// Optional sub-account identifier (appended to username as `username-subN`).
    pub sub_account: Option<String>,
    /// Base path on the StorageBox for storing files.
    pub base_path: String,
}

impl HetznerStorageBoxConfig {
    /// Effective username, including sub-account suffix if configured.
    fn effective_username(&self) -> String {
        match &self.sub_account {
            Some(sub) => format!("{}-{}", self.username, sub),
            None => self.username.clone(),
        }
    }
}

/// Hetzner StorageBox backend via WebDAV protocol.
///
/// Uses reqwest with WebDAV methods (PUT, GET, DELETE, PROPFIND, MKCOL) to
/// interact with Hetzner StorageBox. Each operation creates a fresh HTTP request.
/// Returns `None` from `generate_temp_url()` — access is proxied through the
/// web service.
pub struct HetznerStorageBoxBackend {
    config: HetznerStorageBoxConfig,
    client: reqwest::Client,
}

impl HetznerStorageBoxBackend {
    pub fn new(config: HetznerStorageBoxConfig) -> Self {
        let client = reqwest::Client::new();
        Self { config, client }
    }

    /// Build the full WebDAV URL for a given storage path.
    fn webdav_url(&self, path: &str) -> String {
        let base = self.config.base_path.trim_end_matches('/');
        let file_path = if base.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", base, path)
        };
        format!(
            "https://{}:{}/{}",
            self.config.host, self.config.port, file_path
        )
    }

    /// Build the WebDAV URL for a directory path (with trailing slash).
    fn webdav_dir_url(&self, dir_path: &str) -> String {
        let url = self.webdav_url(dir_path);
        if url.ends_with('/') {
            url
        } else {
            format!("{}/", url)
        }
    }

    /// Create a single directory via MKCOL. Returns Ok for 201/405/301 (created/exists).
    async fn mkcol(&self, raw_url: &str) -> AppResult<()> {
        let resp = self
            .client
            .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), raw_url)
            .basic_auth(
                self.config.effective_username(),
                Some(&self.config.password),
            )
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Hetzner MKCOL failed: {}", e)))?;

        let _status = resp.status().as_u16();
        // 201 = created, 405 = already exists, 301 = redirect (exists)
        // Ignore all — if it really fails, the subsequent PUT will tell us.
        Ok(())
    }

    /// Ensure the base_path directory tree exists on the StorageBox.
    async fn ensure_base_path(&self) -> AppResult<()> {
        let base = self.config.base_path.trim_end_matches('/');
        if base.is_empty() {
            return Ok(());
        }
        let parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
        let mut current = String::new();
        for part in parts {
            if current.is_empty() {
                current = part.to_string();
            } else {
                current = format!("{}/{}", current, part);
            }
            // Build URL directly (not via webdav_dir_url which prepends base_path)
            let url = format!(
                "https://{}:{}/{}/",
                self.config.host, self.config.port, current
            );
            self.mkcol(&url).await?;
        }
        Ok(())
    }

    /// Auto-create directory structure for the given file path using MKCOL.
    /// Creates each component progressively (like `mkdir -p`).
    /// Also ensures the base_path itself exists.
    async fn ensure_parent_dirs(&self, path: &str) -> AppResult<()> {
        // First ensure base_path exists
        self.ensure_base_path().await?;

        // Then create subdirs within base_path for the file
        if let Some((parent, _)) = path.rsplit_once('/') {
            if !parent.is_empty() {
                let parts: Vec<&str> = parent.split('/').filter(|p| !p.is_empty()).collect();
                let mut current = String::new();
                for part in parts {
                    if current.is_empty() {
                        current = part.to_string();
                    } else {
                        current = format!("{}/{}", current, part);
                    }
                    let url = self.webdav_dir_url(&current);
                    self.mkcol(&url).await?;
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl StorageBackend for HetznerStorageBoxBackend {
    async fn upload(&self, path: &str, data: Bytes) -> AppResult<()> {
        // Ensure parent directory structure exists
        self.ensure_parent_dirs(path).await?;

        let url = self.webdav_url(path);
        let resp = self
            .client
            .put(&url)
            .basic_auth(
                self.config.effective_username(),
                Some(&self.config.password),
            )
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Hetzner upload failed: {}", e)))?;

        let status = resp.status();
        if status.as_u16() == 409 {
            // 409 Conflict on PUT means parent directory doesn't exist.
            // Ensure directories and retry once.
            self.ensure_parent_dirs(path).await?;
            let retry_resp = self
                .client
                .put(&url)
                .basic_auth(
                    self.config.effective_username(),
                    Some(&self.config.password),
                )
                .body(data.to_vec())
                .send()
                .await
                .map_err(|e| AppError::Internal(format!("Hetzner upload retry failed: {}", e)))?;
            let retry_status = retry_resp.status();
            if !retry_status.is_success() && retry_status.as_u16() != 201 && retry_status.as_u16() != 204 {
                let body = retry_resp.text().await.unwrap_or_default();
                return Err(AppError::Internal(format!(
                    "Hetzner upload retry returned {}: {}",
                    retry_status, body
                )));
            }
        } else if !status.is_success() && status.as_u16() != 201 && status.as_u16() != 204 {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Hetzner upload returned {}: {}",
                status, body
            )));
        }

        Ok(())
    }

    async fn download(&self, path: &str) -> AppResult<Bytes> {
        let url = self.webdav_url(path);
        let resp = self
            .client
            .get(&url)
            .basic_auth(
                self.config.effective_username(),
                Some(&self.config.password),
            )
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Hetzner download failed: {}", e)))?;

        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(AppError::NotFound(format!(
                "File not found on Hetzner StorageBox: {}",
                path
            )));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Hetzner download returned {}: {}",
                status, body
            )));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::Internal(format!("Hetzner read body failed: {}", e)))?;

        Ok(bytes)
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let url = self.webdav_url(path);
        let resp = self
            .client
            .delete(&url)
            .basic_auth(
                self.config.effective_username(),
                Some(&self.config.password),
            )
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Hetzner delete failed: {}", e)))?;

        let status = resp.status();
        // 204 = deleted, 404 = already gone (idempotent delete)
        if !status.is_success() && status.as_u16() != 404 {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Hetzner delete returned {}: {}",
                status, body
            )));
        }

        Ok(())
    }

    async fn exists(&self, path: &str) -> AppResult<bool> {
        let url = self.webdav_url(path);
        let resp = self
            .client
            .request(
                reqwest::Method::from_bytes(b"PROPFIND").unwrap(),
                &url,
            )
            .basic_auth(
                self.config.effective_username(),
                Some(&self.config.password),
            )
            .header("Depth", "0")
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Hetzner PROPFIND failed: {}", e)))?;

        let status = resp.status().as_u16();
        // 207 Multi-Status = exists, 404 = not found
        Ok(status == 207)
    }

    async fn generate_temp_url(
        &self,
        _path: &str,
        _expires_in: Duration,
        _filename: Option<&str>,
    ) -> AppResult<Option<String>> {
        // Hetzner StorageBox does not support direct URL access;
        // downloads are proxied through the web service.
        Ok(None)
    }

    async fn list(&self, prefix: &str) -> AppResult<Vec<String>> {
        let dir = if prefix.is_empty() {
            String::new()
        } else {
            prefix.to_string()
        };
        let url = self.webdav_dir_url(&dir);

        let propfind_body = r#"<?xml version="1.0" encoding="utf-8" ?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
    <D:displayname/>
    <D:resourcetype/>
  </D:prop>
</D:propfind>"#;

        let resp = self
            .client
            .request(
                reqwest::Method::from_bytes(b"PROPFIND").unwrap(),
                &url,
            )
            .basic_auth(
                self.config.effective_username(),
                Some(&self.config.password),
            )
            .header("Depth", "1")
            .header("Content-Type", "application/xml")
            .body(propfind_body)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Hetzner PROPFIND list failed: {}", e)))?;

        let status = resp.status().as_u16();
        if status == 404 {
            return Ok(Vec::new());
        }
        if status != 207 {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Hetzner PROPFIND list returned {}: {}",
                status, body
            )));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| AppError::Internal(format!("Hetzner read PROPFIND body failed: {}", e)))?;

        // Parse href elements from the PROPFIND XML response.
        // We do a simple text scan for <D:href> or <d:href> tags rather than
        // pulling in a full XML parser.
        let entries = parse_propfind_hrefs(&body, &self.config.base_path);

        Ok(entries)
    }

    async fn create_container(&self, name: &str) -> AppResult<()> {
        let url = self.webdav_dir_url(name);
        let resp = self
            .client
            .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), &url)
            .basic_auth(
                self.config.effective_username(),
                Some(&self.config.password),
            )
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Hetzner MKCOL failed: {}", e)))?;

        let status = resp.status().as_u16();
        // 201 = created, 405 = already exists, 301 = redirect (exists)
        if status == 201 || status == 405 || status == 301 {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(AppError::Internal(format!(
                "Hetzner create_container returned {}: {}",
                status, body
            )))
        }
    }

    fn supports_containers(&self) -> bool {
        true
    }
}

/// Extract file paths from a PROPFIND multi-status XML response.
///
/// Parses `<D:href>` (or `<d:href>`) elements, strips the base path prefix,
/// and filters out directory entries (those ending with `/`).
fn parse_propfind_hrefs(xml: &str, base_path: &str) -> Vec<String> {
    let mut results = Vec::new();
    let lower = xml.to_lowercase();
    let mut search_start = 0;

    loop {
        // Find opening tag (case-insensitive)
        let open_tag = if let Some(pos) = lower[search_start..].find("<d:href>") {
            search_start + pos + 8
        } else if let Some(pos) = lower[search_start..].find("<href>") {
            search_start + pos + 6
        } else {
            break;
        };

        // Find closing tag
        let close_tag = if let Some(pos) = lower[open_tag..].find("</d:href>") {
            open_tag + pos
        } else if let Some(pos) = lower[open_tag..].find("</href>") {
            open_tag + pos
        } else {
            break;
        };

        // Extract the actual (non-lowered) href value
        let href = xml[open_tag..close_tag].trim();
        search_start = close_tag + 1;

        // Skip directory entries (trailing slash)
        if href.ends_with('/') {
            continue;
        }

        // Decode percent-encoded characters
        let decoded = urlencoding::decode(href).unwrap_or_else(|_| href.into());

        // Strip base path prefix
        let base = base_path.trim_matches('/');
        let path = decoded.trim_start_matches('/');
        let stripped = if !base.is_empty() {
            let prefix = format!("{}/", base);
            path.strip_prefix(&prefix).unwrap_or(path)
        } else {
            path
        };

        if !stripped.is_empty() {
            results.push(stripped.to_string());
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> HetznerStorageBoxConfig {
        HetznerStorageBoxConfig {
            host: "u123456.your-storagebox.de".to_string(),
            port: 443,
            username: "u123456".to_string(),
            password: "secretpass".to_string(),
            sub_account: None,
            base_path: "".to_string(),
        }
    }

    fn test_config_with_base_path() -> HetznerStorageBoxConfig {
        HetznerStorageBoxConfig {
            base_path: "storage/files".to_string(),
            ..test_config()
        }
    }

    fn test_config_with_sub_account() -> HetznerStorageBoxConfig {
        HetznerStorageBoxConfig {
            sub_account: Some("sub1".to_string()),
            ..test_config()
        }
    }

    #[test]
    fn test_effective_username_no_sub_account() {
        let config = test_config();
        assert_eq!(config.effective_username(), "u123456");
    }

    #[test]
    fn test_effective_username_with_sub_account() {
        let config = test_config_with_sub_account();
        assert_eq!(config.effective_username(), "u123456-sub1");
    }

    #[test]
    fn test_webdav_url_no_base_path() {
        let backend = HetznerStorageBoxBackend::new(test_config());
        assert_eq!(
            backend.webdav_url("ab/cd/abcdef123456"),
            "https://u123456.your-storagebox.de:443/ab/cd/abcdef123456"
        );
    }

    #[test]
    fn test_webdav_url_with_base_path() {
        let backend = HetznerStorageBoxBackend::new(test_config_with_base_path());
        assert_eq!(
            backend.webdav_url("ab/cd/abcdef123456"),
            "https://u123456.your-storagebox.de:443/storage/files/ab/cd/abcdef123456"
        );
    }

    #[test]
    fn test_webdav_url_trailing_slash_base_path() {
        let config = HetznerStorageBoxConfig {
            base_path: "storage/files/".to_string(),
            ..test_config()
        };
        let backend = HetznerStorageBoxBackend::new(config);
        assert_eq!(
            backend.webdav_url("test.txt"),
            "https://u123456.your-storagebox.de:443/storage/files/test.txt"
        );
    }

    #[test]
    fn test_webdav_dir_url() {
        let backend = HetznerStorageBoxBackend::new(test_config());
        assert_eq!(
            backend.webdav_dir_url("ab/cd"),
            "https://u123456.your-storagebox.de:443/ab/cd/"
        );
    }

    #[test]
    fn test_webdav_dir_url_already_trailing_slash() {
        let backend = HetznerStorageBoxBackend::new(test_config());
        let url = backend.webdav_dir_url("ab/cd/");
        assert!(url.ends_with('/'));
        // Should not double the slash
        assert!(!url.ends_with("//"));
    }

    #[test]
    fn test_custom_port() {
        let config = HetznerStorageBoxConfig {
            port: 9090,
            ..test_config()
        };
        let backend = HetznerStorageBoxBackend::new(config);
        assert_eq!(
            backend.webdav_url("file.txt"),
            "https://u123456.your-storagebox.de:9090/file.txt"
        );
    }

    #[tokio::test]
    async fn test_generate_temp_url_returns_none() {
        let backend = HetznerStorageBoxBackend::new(test_config());
        let result = backend
            .generate_temp_url("test.txt", Duration::from_secs(3600), None)
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_config_clone() {
        let config = test_config_with_sub_account();
        let cloned = config.clone();
        assert_eq!(cloned.host, "u123456.your-storagebox.de");
        assert_eq!(cloned.port, 443);
        assert_eq!(cloned.username, "u123456");
        assert_eq!(cloned.sub_account, Some("sub1".to_string()));
    }

    #[test]
    fn test_config_debug() {
        let config = test_config();
        let debug = format!("{:?}", config);
        assert!(debug.contains("u123456.your-storagebox.de"));
        assert!(debug.contains("443"));
    }

    #[test]
    fn test_parse_propfind_hrefs_basic() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/storage/</D:href>
    <D:propstat>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/storage/ab/cd/abcdef123456</D:href>
    <D:propstat>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/storage/ef/gh/efgh789</D:href>
    <D:propstat>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

        let results = parse_propfind_hrefs(xml, "storage");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], "ab/cd/abcdef123456");
        assert_eq!(results[1], "ef/gh/efgh789");
    }

    #[test]
    fn test_parse_propfind_hrefs_no_base_path() {
        let xml = r#"<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/</D:href>
  </D:response>
  <D:response>
    <D:href>/file1.txt</D:href>
  </D:response>
  <D:response>
    <D:href>/subdir/</D:href>
  </D:response>
  <D:response>
    <D:href>/file2.txt</D:href>
  </D:response>
</D:multistatus>"#;

        let results = parse_propfind_hrefs(xml, "");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], "file1.txt");
        assert_eq!(results[1], "file2.txt");
    }

    #[test]
    fn test_parse_propfind_hrefs_filters_directories() {
        let xml = r#"<D:multistatus xmlns:D="DAV:">
  <D:response><D:href>/base/dir1/</D:href></D:response>
  <D:response><D:href>/base/dir2/</D:href></D:response>
  <D:response><D:href>/base/file.bin</D:href></D:response>
</D:multistatus>"#;

        let results = parse_propfind_hrefs(xml, "base");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "file.bin");
    }

    #[test]
    fn test_parse_propfind_hrefs_empty_response() {
        let xml = r#"<D:multistatus xmlns:D="DAV:"></D:multistatus>"#;
        let results = parse_propfind_hrefs(xml, "");
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_propfind_hrefs_percent_encoded() {
        let xml = r#"<D:multistatus xmlns:D="DAV:">
  <D:response><D:href>/data/file%20name.txt</D:href></D:response>
</D:multistatus>"#;

        let results = parse_propfind_hrefs(xml, "data");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "file name.txt");
    }

    #[tokio::test]
    async fn test_upload_to_nonexistent_host_returns_error() {
        let config = HetznerStorageBoxConfig {
            host: "nonexistent.invalid".to_string(),
            port: 19990,
            ..test_config()
        };
        let backend = HetznerStorageBoxBackend::new(config);

        let result = backend.upload("test.txt", Bytes::from("hello")).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::Internal(msg) => assert!(msg.contains("Hetzner")),
            other => panic!("Expected Internal error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_download_from_nonexistent_host_returns_error() {
        let config = HetznerStorageBoxConfig {
            host: "nonexistent.invalid".to_string(),
            port: 19990,
            ..test_config()
        };
        let backend = HetznerStorageBoxBackend::new(config);

        let result = backend.download("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_from_nonexistent_host_returns_error() {
        let config = HetznerStorageBoxConfig {
            host: "nonexistent.invalid".to_string(),
            port: 19990,
            ..test_config()
        };
        let backend = HetznerStorageBoxBackend::new(config);

        let result = backend.delete("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_exists_on_nonexistent_host_returns_error() {
        let config = HetznerStorageBoxConfig {
            host: "nonexistent.invalid".to_string(),
            port: 19990,
            ..test_config()
        };
        let backend = HetznerStorageBoxBackend::new(config);

        let result = backend.exists("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_on_nonexistent_host_returns_error() {
        let config = HetznerStorageBoxConfig {
            host: "nonexistent.invalid".to_string(),
            port: 19990,
            ..test_config()
        };
        let backend = HetznerStorageBoxBackend::new(config);

        let result = backend.list("").await;
        assert!(result.is_err());
    }
}
