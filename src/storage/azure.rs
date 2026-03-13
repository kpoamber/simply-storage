use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::storage::traits::StorageBackend;

type HmacSha256 = Hmac<Sha256>;

const API_VERSION: &str = "2020-10-02";

/// Configuration for Azure Blob Storage backend.
#[derive(Debug, Clone)]
pub struct AzureBlobConfig {
    /// Azure storage account name.
    pub account_name: String,
    /// Azure storage account key (base64-encoded).
    pub account_key: String,
    /// Blob container name.
    pub container: String,
    /// Optional prefix (virtual directory) for all blobs.
    pub prefix: String,
    /// Optional custom endpoint URL (for Azurite emulator or sovereign clouds).
    pub endpoint: Option<String>,
}

/// Azure Blob Storage backend using the REST API with SharedKey authentication.
pub struct AzureBlobBackend {
    client: reqwest::Client,
    account_name: String,
    account_key: Vec<u8>,
    container: String,
    prefix: String,
    base_url: String,
}

impl AzureBlobBackend {
    /// Create a new Azure Blob Storage backend from configuration.
    pub fn new(config: AzureBlobConfig) -> AppResult<Self> {
        let account_key = BASE64
            .decode(&config.account_key)
            .map_err(|e| AppError::Internal(format!("Invalid Azure account key: {}", e)))?;

        let base_url = config.endpoint.unwrap_or_else(|| {
            format!(
                "https://{}.blob.core.windows.net",
                config.account_name
            )
        });

        Ok(Self {
            client: reqwest::Client::new(),
            account_name: config.account_name,
            account_key,
            container: config.container,
            prefix: config.prefix,
            base_url,
        })
    }

    /// Build the full blob path from a logical storage path, applying the prefix.
    fn blob_path(&self, path: &str) -> String {
        if self.prefix.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", self.prefix.trim_end_matches('/'), path)
        }
    }

    /// Build the full URL for a blob.
    fn blob_url(&self, blob_path: &str) -> String {
        format!("{}/{}/{}", self.base_url, self.container, blob_path)
    }

    /// HMAC-SHA256 sign a string and return the base64-encoded signature.
    fn hmac_sign(&self, data: &str) -> AppResult<String> {
        let mut mac = HmacSha256::new_from_slice(&self.account_key)
            .map_err(|e| AppError::Internal(format!("HMAC init error: {}", e)))?;
        mac.update(data.as_bytes());
        Ok(BASE64.encode(mac.finalize().into_bytes()))
    }

    /// Generate the SharedKey Authorization header value.
    #[allow(clippy::too_many_arguments)]
    fn shared_key_auth(
        &self,
        verb: &str,
        resource_path: &str,
        date: &str,
        content_length: Option<usize>,
        content_type: &str,
        extra_ms_headers: &[(&str, &str)],
        query_params: &[(&str, &str)],
    ) -> AppResult<String> {
        let cl_str = match content_length {
            Some(0) | None => String::new(),
            Some(n) => n.to_string(),
        };

        // Build canonicalized headers (sorted x-ms-* headers)
        let mut ms_headers: Vec<(String, String)> = vec![
            ("x-ms-date".into(), date.into()),
            ("x-ms-version".into(), API_VERSION.into()),
        ];
        for &(k, v) in extra_ms_headers {
            if k.starts_with("x-ms-") {
                ms_headers.push((k.to_string(), v.to_string()));
            }
        }
        ms_headers.sort_by(|a, b| a.0.cmp(&b.0));

        let canonical_headers: String = ms_headers
            .iter()
            .map(|(k, v)| format!("{}:{}\n", k, v))
            .collect();

        // Build canonicalized resource
        let mut canonical_resource = format!("/{}/{}", self.account_name, resource_path);
        if !query_params.is_empty() {
            let mut sorted: Vec<_> = query_params.to_vec();
            sorted.sort_by_key(|(k, _)| k.to_string());
            for (k, v) in &sorted {
                canonical_resource.push_str(&format!("\n{}:{}", k, v));
            }
        }

        // String to sign (Blob service, SharedKey)
        // VERB\nContent-Encoding\nContent-Language\nContent-Length\nContent-MD5\nContent-Type\n
        // Date\nIf-Modified-Since\nIf-Match\nIf-None-Match\nIf-Unmodified-Since\nRange\n
        // CanonicalizedHeaders CanonicalizedResource
        let string_to_sign = format!(
            "{}\n\n\n{}\n\n{}\n\n\n\n\n\n\n{}{}",
            verb, cl_str, content_type, canonical_headers, canonical_resource
        );

        let sig = self.hmac_sign(&string_to_sign)?;
        Ok(format!("SharedKey {}:{}", self.account_name, sig))
    }

    /// Generate a Service SAS URL for read access to a blob.
    fn generate_sas_url(&self, blob_path: &str, expires_in: Duration) -> AppResult<String> {
        let now = Utc::now();
        let expiry = now
            + chrono::Duration::from_std(expires_in)
                .map_err(|e| AppError::Internal(format!("Duration error: {}", e)))?;

        let st = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let se = expiry.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let sp = "r";
        let sr = "b";
        let sv = API_VERSION;

        let canonical_resource = format!(
            "/blob/{}/{}/{}",
            self.account_name, self.container, blob_path
        );

        // String to sign for Service SAS (version 2020-10-02)
        // sp\nst\nse\ncanonicalizedResource\nidentifier\nIP\nprotocol\nversion\nresource\n
        // snapshot\nencryptionScope\nrscc\nrscd\nrsce\nrscl\nrsct
        let string_to_sign = format!(
            "{}\n{}\n{}\n{}\n\n\n{}\n{}\n\n\n\n\n\n",
            sp, st, se, canonical_resource, sv, sr,
        );

        let sig = self.hmac_sign(&string_to_sign)?;

        let url = format!(
            "{}?sp={}&st={}&se={}&sv={}&sr={}&sig={}",
            self.blob_url(blob_path),
            sp,
            urlencoding::encode(&st),
            urlencoding::encode(&se),
            sv,
            sr,
            urlencoding::encode(&sig),
        );

        Ok(url)
    }
}

impl AzureBlobBackend {
    /// List all containers in this Azure storage account.
    pub async fn list_account_containers(&self) -> AppResult<Vec<String>> {
        let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();

        let query_params: Vec<(&str, &str)> = vec![("comp", "list")];
        let auth = self.shared_key_auth(
            "GET",
            "",
            &date,
            None,
            "",
            &[],
            &query_params,
        )?;

        let url = format!("{}/?comp=list", self.base_url);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", auth)
            .header("x-ms-date", &date)
            .header("x-ms-version", API_VERSION)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Azure list containers failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Azure list containers failed with status {}: {}",
                status, body
            )));
        }

        let body = resp.text().await.map_err(|e| {
            AppError::Internal(format!("Azure list containers body read failed: {}", e))
        })?;

        let mut containers = Vec::new();
        for section in body.split("<Container>").skip(1) {
            if let Some(name_start) = section.find("<Name>") {
                if let Some(name_end) = section.find("</Name>") {
                    let name = &section[name_start + 6..name_end];
                    containers.push(name.to_string());
                }
            }
        }

        Ok(containers)
    }

    /// Perform a single blob upload attempt, returning the raw response.
    async fn upload_blob(&self, blob_path: &str, data: &Bytes) -> AppResult<reqwest::Response> {
        let url = self.blob_url(blob_path);
        let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        let content_length = data.len();

        let resource_path = format!("{}/{}", self.container, blob_path);
        let auth = self.shared_key_auth(
            "PUT",
            &resource_path,
            &date,
            Some(content_length),
            "application/octet-stream",
            &[("x-ms-blob-type", "BlockBlob")],
            &[],
        )?;

        self.client
            .put(&url)
            .header("Authorization", auth)
            .header("x-ms-date", &date)
            .header("x-ms-version", API_VERSION)
            .header("x-ms-blob-type", "BlockBlob")
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Azure upload failed: {}", e)))
    }

    /// Create a new container in this Azure storage account.
    pub async fn create_account_container(&self, name: &str) -> AppResult<()> {
        let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();

        let resource_path = name.to_string();
        let query_params: Vec<(&str, &str)> = vec![("restype", "container")];
        let auth = self.shared_key_auth(
            "PUT",
            &resource_path,
            &date,
            Some(0),
            "",
            &[],
            &query_params,
        )?;

        let url = format!("{}/{}?restype=container", self.base_url, name);

        let resp = self
            .client
            .put(&url)
            .header("Authorization", auth)
            .header("x-ms-date", &date)
            .header("x-ms-version", API_VERSION)
            .header("Content-Length", "0")
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Azure create container failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Azure create container failed with status {}: {}",
                status, body
            )));
        }

        Ok(())
    }
}

#[async_trait]
impl StorageBackend for AzureBlobBackend {
    async fn upload(&self, path: &str, data: Bytes) -> AppResult<()> {
        let bp = self.blob_path(path);

        let resp = self.upload_blob(&bp, &data).await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();

            // Auto-create container if it doesn't exist, then retry
            if body.contains("ContainerNotFound") {
                tracing::info!(container = %self.container, "Container not found, creating automatically");
                self.create_account_container(&self.container.clone()).await?;

                let retry_resp = self.upload_blob(&bp, &data).await?;
                if !retry_resp.status().is_success() {
                    let retry_status = retry_resp.status();
                    let retry_body = retry_resp.text().await.unwrap_or_default();
                    return Err(AppError::Internal(format!(
                        "Azure upload failed with status {}: {}",
                        retry_status, retry_body
                    )));
                }
                return Ok(());
            }

            return Err(AppError::Internal(format!(
                "Azure upload failed with status {}: {}",
                status, body
            )));
        }

        Ok(())
    }

    async fn download(&self, path: &str) -> AppResult<Bytes> {
        let bp = self.blob_path(path);
        let url = self.blob_url(&bp);
        let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();

        let resource_path = format!("{}/{}", self.container, bp);
        let auth = self.shared_key_auth("GET", &resource_path, &date, None, "", &[], &[])?;

        let resp = self
            .client
            .get(&url)
            .header("Authorization", auth)
            .header("x-ms-date", &date)
            .header("x-ms-version", API_VERSION)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Azure download failed: {}", e)))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::NotFound(format!(
                "File not found in Azure: {}",
                path
            )));
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Azure download failed with status {}: {}",
                status, body
            )));
        }

        resp.bytes()
            .await
            .map_err(|e| AppError::Internal(format!("Azure download body read failed: {}", e)))
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let bp = self.blob_path(path);
        let url = self.blob_url(&bp);
        let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();

        let resource_path = format!("{}/{}", self.container, bp);
        let auth = self.shared_key_auth("DELETE", &resource_path, &date, None, "", &[], &[])?;

        let resp = self
            .client
            .delete(&url)
            .header("Authorization", auth)
            .header("x-ms-date", &date)
            .header("x-ms-version", API_VERSION)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Azure delete failed: {}", e)))?;

        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Azure delete failed with status {}: {}",
                status, body
            )));
        }

        Ok(())
    }

    async fn exists(&self, path: &str) -> AppResult<bool> {
        let bp = self.blob_path(path);
        let url = self.blob_url(&bp);
        let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();

        let resource_path = format!("{}/{}", self.container, bp);
        let auth = self.shared_key_auth("HEAD", &resource_path, &date, None, "", &[], &[])?;

        let resp = self
            .client
            .head(&url)
            .header("Authorization", auth)
            .header("x-ms-date", &date)
            .header("x-ms-version", API_VERSION)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Azure head failed: {}", e)))?;

        match resp.status() {
            s if s.is_success() => Ok(true),
            reqwest::StatusCode::NOT_FOUND => Ok(false),
            status => Err(AppError::Internal(format!(
                "Azure head failed with status {}",
                status
            ))),
        }
    }

    async fn generate_temp_url(
        &self,
        path: &str,
        expires_in: Duration,
    ) -> AppResult<Option<String>> {
        let bp = self.blob_path(path);
        let url = self.generate_sas_url(&bp, expires_in)?;
        Ok(Some(url))
    }

    async fn list_containers(&self) -> AppResult<Vec<String>> {
        self.list_account_containers().await
    }

    async fn create_container(&self, name: &str) -> AppResult<()> {
        self.create_account_container(name).await
    }

    fn supports_containers(&self) -> bool {
        true
    }

    async fn list(&self, prefix: &str) -> AppResult<Vec<String>> {
        let full_prefix = self.blob_path(prefix);
        let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();

        let query_params: Vec<(&str, &str)> = vec![
            ("comp", "list"),
            ("prefix", &full_prefix),
            ("restype", "container"),
        ];

        let resource_path = self.container.clone();
        let auth =
            self.shared_key_auth("GET", &resource_path, &date, None, "", &[], &query_params)?;

        let url = format!(
            "{}/{}?restype=container&comp=list&prefix={}",
            self.base_url,
            self.container,
            urlencoding::encode(&full_prefix),
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", auth)
            .header("x-ms-date", &date)
            .header("x-ms-version", API_VERSION)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Azure list failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Azure list failed with status {}: {}",
                status, body
            )));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| AppError::Internal(format!("Azure list body read failed: {}", e)))?;

        // Parse XML response - extract <Name> elements within <Blob> sections
        let mut results = Vec::new();
        for blob_section in body.split("<Blob>").skip(1) {
            if let Some(name_start) = blob_section.find("<Name>") {
                if let Some(name_end) = blob_section.find("</Name>") {
                    let name = &blob_section[name_start + 6..name_end];
                    let relative = if !self.prefix.is_empty() {
                        let prefix_with_slash =
                            format!("{}/", self.prefix.trim_end_matches('/'));
                        name.strip_prefix(&prefix_with_slash)
                            .unwrap_or(name)
                            .to_string()
                    } else {
                        name.to_string()
                    };
                    results.push(relative);
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AzureBlobConfig {
        // Use a valid base64-encoded key for testing
        let fake_key = BASE64.encode(b"fake-azure-account-key-for-tests!");
        AzureBlobConfig {
            account_name: "testaccount".to_string(),
            account_key: fake_key,
            container: "testcontainer".to_string(),
            prefix: "".to_string(),
            endpoint: Some("http://127.0.0.1:10000/testaccount".to_string()),
        }
    }

    fn test_config_with_prefix() -> AzureBlobConfig {
        AzureBlobConfig {
            prefix: "my-prefix".to_string(),
            ..test_config()
        }
    }

    #[test]
    fn test_blob_path_no_prefix() {
        let backend = AzureBlobBackend::new(test_config()).unwrap();
        assert_eq!(backend.blob_path("ab/cd/abcdef123"), "ab/cd/abcdef123");
    }

    #[test]
    fn test_blob_path_with_prefix() {
        let backend = AzureBlobBackend::new(test_config_with_prefix()).unwrap();
        assert_eq!(
            backend.blob_path("ab/cd/abcdef123"),
            "my-prefix/ab/cd/abcdef123"
        );
    }

    #[test]
    fn test_blob_path_with_trailing_slash_prefix() {
        let config = AzureBlobConfig {
            prefix: "my-prefix/".to_string(),
            ..test_config()
        };
        let backend = AzureBlobBackend::new(config).unwrap();
        assert_eq!(
            backend.blob_path("test.txt"),
            "my-prefix/test.txt"
        );
    }

    #[test]
    fn test_blob_url() {
        let backend = AzureBlobBackend::new(test_config()).unwrap();
        let url = backend.blob_url("path/to/file.txt");
        assert!(url.contains("testcontainer"));
        assert!(url.contains("path/to/file.txt"));
    }

    #[test]
    fn test_invalid_account_key() {
        let config = AzureBlobConfig {
            account_key: "not-valid-base64!!!".to_string(),
            ..test_config()
        };
        let result = AzureBlobBackend::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_default_endpoint() {
        let config = AzureBlobConfig {
            endpoint: None,
            ..test_config()
        };
        let backend = AzureBlobBackend::new(config).unwrap();
        assert_eq!(
            backend.base_url,
            "https://testaccount.blob.core.windows.net"
        );
    }

    #[test]
    fn test_custom_endpoint() {
        let backend = AzureBlobBackend::new(test_config()).unwrap();
        assert_eq!(
            backend.base_url,
            "http://127.0.0.1:10000/testaccount"
        );
    }

    #[test]
    fn test_sas_url_format() {
        let backend = AzureBlobBackend::new(test_config()).unwrap();
        let sas_url = backend
            .generate_sas_url("path/to/file.txt", Duration::from_secs(3600))
            .unwrap();

        assert!(sas_url.contains("path/to/file.txt"));
        assert!(sas_url.contains("sp=r"));
        assert!(sas_url.contains("sr=b"));
        assert!(sas_url.contains(&format!("sv={}", API_VERSION)));
        assert!(sas_url.contains("sig="));
        assert!(sas_url.contains("st="));
        assert!(sas_url.contains("se="));
    }

    #[test]
    fn test_sas_url_with_prefix() {
        let backend = AzureBlobBackend::new(test_config_with_prefix()).unwrap();
        let sas_url = backend
            .generate_sas_url("my-prefix/file.txt", Duration::from_secs(300))
            .unwrap();

        assert!(sas_url.contains("my-prefix/file.txt"));
        assert!(sas_url.contains("sig="));
    }

    #[test]
    fn test_shared_key_auth_format() {
        let backend = AzureBlobBackend::new(test_config()).unwrap();
        let auth = backend
            .shared_key_auth(
                "GET",
                "testcontainer/test.txt",
                "Thu, 12 Mar 2026 17:00:00 GMT",
                None,
                "",
                &[],
                &[],
            )
            .unwrap();

        assert!(auth.starts_with("SharedKey testaccount:"));
        // Signature should be base64-encoded
        let sig = auth.strip_prefix("SharedKey testaccount:").unwrap();
        assert!(BASE64.decode(sig).is_ok());
    }

    #[test]
    fn test_config_clone() {
        let config = test_config();
        let cloned = config.clone();
        assert_eq!(cloned.account_name, "testaccount");
        assert_eq!(cloned.container, "testcontainer");
    }

    #[test]
    fn test_config_debug() {
        let config = test_config();
        let debug = format!("{:?}", config);
        assert!(debug.contains("testaccount"));
        assert!(debug.contains("testcontainer"));
    }

    #[tokio::test]
    async fn test_upload_to_nonexistent_endpoint_returns_error() {
        let config = AzureBlobConfig {
            endpoint: Some("http://127.0.0.1:19998".to_string()),
            ..test_config()
        };
        let backend = AzureBlobBackend::new(config).unwrap();

        let result = backend.upload("test.txt", Bytes::from("hello")).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::Internal(msg) => assert!(msg.contains("Azure upload failed")),
            other => panic!("Expected Internal error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_download_from_nonexistent_endpoint_returns_error() {
        let config = AzureBlobConfig {
            endpoint: Some("http://127.0.0.1:19998".to_string()),
            ..test_config()
        };
        let backend = AzureBlobBackend::new(config).unwrap();

        let result = backend.download("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_from_nonexistent_endpoint_returns_error() {
        let config = AzureBlobConfig {
            endpoint: Some("http://127.0.0.1:19998".to_string()),
            ..test_config()
        };
        let backend = AzureBlobBackend::new(config).unwrap();

        let result = backend.delete("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_exists_on_nonexistent_endpoint_returns_error() {
        let config = AzureBlobConfig {
            endpoint: Some("http://127.0.0.1:19998".to_string()),
            ..test_config()
        };
        let backend = AzureBlobBackend::new(config).unwrap();

        let result = backend.exists("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_on_nonexistent_endpoint_returns_error() {
        let config = AzureBlobConfig {
            endpoint: Some("http://127.0.0.1:19998".to_string()),
            ..test_config()
        };
        let backend = AzureBlobBackend::new(config).unwrap();

        let result = backend.list("").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_generate_temp_url_returns_sas_url() {
        let backend = AzureBlobBackend::new(test_config()).unwrap();
        let result = backend
            .generate_temp_url("test-file.txt", Duration::from_secs(3600))
            .await;

        assert!(result.is_ok());
        let url = result.unwrap();
        assert!(url.is_some());
        let url_str = url.unwrap();
        assert!(url_str.contains("test-file.txt"));
        assert!(url_str.contains("sig="));
    }
}
