use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use rsa::pkcs8::DecodePrivateKey;
use rsa::RsaPrivateKey;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::sync::RwLock;

use crate::error::{AppError, AppResult};
use crate::storage::traits::StorageBackend;

const GCS_UPLOAD_BASE: &str = "https://storage.googleapis.com/upload/storage/v1";
const GCS_API_V1: &str = "https://storage.googleapis.com/storage/v1";
const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const TOKEN_LIFETIME_SECS: i64 = 3600;

/// Configuration for Google Cloud Storage backend.
#[derive(Debug, Clone)]
pub struct GcsConfig {
    /// GCS bucket name.
    pub bucket: String,
    /// Optional prefix for all objects.
    pub prefix: String,
    /// Service account client email.
    pub client_email: String,
    /// Service account private key in PEM format.
    pub private_key_pem: String,
    /// Token URI (defaults to Google's OAuth2 endpoint).
    pub token_uri: Option<String>,
}

struct CachedToken {
    token: String,
    expires_at: chrono::DateTime<Utc>,
}

/// Google Cloud Storage backend using the JSON API with service account authentication.
pub struct GcsBackend {
    client: reqwest::Client,
    bucket: String,
    prefix: String,
    client_email: String,
    private_key: RsaPrivateKey,
    token_uri: String,
    cached_token: RwLock<Option<CachedToken>>,
}

impl GcsBackend {
    /// Create a new GCS backend from configuration.
    pub fn new(config: GcsConfig) -> AppResult<Self> {
        let private_key = RsaPrivateKey::from_pkcs8_pem(&config.private_key_pem)
            .map_err(|e| AppError::Internal(format!("Invalid GCS private key: {}", e)))?;

        let token_uri = config.token_uri.unwrap_or_else(|| TOKEN_URI.to_string());

        Ok(Self {
            client: reqwest::Client::new(),
            bucket: config.bucket,
            prefix: config.prefix,
            client_email: config.client_email,
            private_key,
            token_uri,
            cached_token: RwLock::new(None),
        })
    }

    /// Build the full object path from a logical storage path.
    fn object_path(&self, path: &str) -> String {
        if self.prefix.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", self.prefix.trim_end_matches('/'), path)
        }
    }

    /// RSA-SHA256 sign data using the service account private key.
    fn rsa_sign(&self, data: &[u8]) -> AppResult<Vec<u8>> {
        use rsa::pkcs1v15::SigningKey;
        use rsa::signature::{SignatureEncoding, Signer};

        let signing_key = SigningKey::<Sha256>::new(self.private_key.clone());
        let signature = signing_key.sign(data);
        Ok(signature.to_vec())
    }

    /// Create a JWT for OAuth2 token exchange.
    fn create_jwt(&self) -> AppResult<String> {
        let now = Utc::now().timestamp();
        let exp = now + TOKEN_LIFETIME_SECS;

        let header = BASE64URL.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let payload_json = format!(
            r#"{{"iss":"{}","scope":"https://www.googleapis.com/auth/devstorage.full_control","aud":"{}","iat":{},"exp":{}}}"#,
            self.client_email, self.token_uri, now, exp
        );
        let payload = BASE64URL.encode(payload_json.as_bytes());

        let unsigned = format!("{}.{}", header, payload);
        let signature = self.rsa_sign(unsigned.as_bytes())?;
        let sig_encoded = BASE64URL.encode(&signature);

        Ok(format!("{}.{}", unsigned, sig_encoded))
    }

    /// Get a valid access token, refreshing if needed.
    async fn get_access_token(&self) -> AppResult<String> {
        // Check cached token
        {
            let cache = self.cached_token.read().await;
            if let Some(ref token) = *cache {
                if token.expires_at > Utc::now() + chrono::Duration::seconds(60) {
                    return Ok(token.token.clone());
                }
            }
        }

        // Create JWT and exchange for access token
        let jwt = self.create_jwt()?;

        let resp = self
            .client
            .post(&self.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &jwt),
            ])
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("GCS token exchange failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "GCS token exchange failed with status {}: {}",
                status, body
            )));
        }

        let token_resp: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("GCS token response parse failed: {}", e)))?;

        let access_token = token_resp["access_token"]
            .as_str()
            .ok_or_else(|| AppError::Internal("No access_token in GCS response".into()))?
            .to_string();

        let expires_in = token_resp["expires_in"].as_i64().unwrap_or(TOKEN_LIFETIME_SECS);

        // Cache the token
        let mut cache = self.cached_token.write().await;
        *cache = Some(CachedToken {
            token: access_token.clone(),
            expires_at: Utc::now() + chrono::Duration::seconds(expires_in),
        });

        Ok(access_token)
    }

    /// Generate a V4 signed URL using the service account private key.
    fn generate_signed_url_v4(
        &self,
        object_path: &str,
        expires_in: Duration,
    ) -> AppResult<String> {
        let now = Utc::now();
        let datetime = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let expires_secs = expires_in.as_secs();

        let credential_scope = format!("{}/auto/storage/goog4_request", date);
        let credential = format!("{}/{}", self.client_email, credential_scope);

        let host = "storage.googleapis.com";
        let path = format!("/{}/{}", self.bucket, urlencoding::encode(object_path));

        // Build canonical query string (sorted)
        let mut query_params: Vec<(String, String)> = vec![
            ("X-Goog-Algorithm".into(), "GOOG4-RSA-SHA256".into()),
            ("X-Goog-Credential".into(), credential.clone()),
            ("X-Goog-Date".into(), datetime.clone()),
            ("X-Goog-Expires".into(), expires_secs.to_string()),
            ("X-Goog-SignedHeaders".into(), "host".into()),
        ];
        query_params.sort_by(|a, b| a.0.cmp(&b.0));

        let canonical_query = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        // Canonical request
        let canonical_request = format!(
            "GET\n{}\n{}\nhost:{}\n\nhost\nUNSIGNED-PAYLOAD",
            path, canonical_query, host
        );

        // String to sign
        let hashed_request = hex::encode(Sha256::digest(canonical_request.as_bytes()));
        let string_to_sign = format!(
            "GOOG4-RSA-SHA256\n{}\n{}\n{}",
            datetime, credential_scope, hashed_request
        );

        // Sign with RSA-SHA256
        let signature = self.rsa_sign(string_to_sign.as_bytes())?;
        let hex_sig = hex::encode(&signature);

        Ok(format!(
            "https://{}{}?{}&X-Goog-Signature={}",
            host, path, canonical_query, hex_sig
        ))
    }
}

#[async_trait]
impl StorageBackend for GcsBackend {
    async fn upload(&self, path: &str, data: Bytes) -> AppResult<()> {
        let object = self.object_path(path);
        let token = self.get_access_token().await?;

        let url = format!(
            "{}/b/{}/o?uploadType=media&name={}",
            GCS_UPLOAD_BASE,
            urlencoding::encode(&self.bucket),
            urlencoding::encode(&object),
        );

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("GCS upload failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "GCS upload failed with status {}: {}",
                status, body
            )));
        }

        Ok(())
    }

    async fn download(&self, path: &str) -> AppResult<Bytes> {
        let object = self.object_path(path);
        let token = self.get_access_token().await?;

        let url = format!(
            "{}/b/{}/o/{}?alt=media",
            GCS_API_V1,
            urlencoding::encode(&self.bucket),
            urlencoding::encode(&object),
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("GCS download failed: {}", e)))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::NotFound(format!(
                "File not found in GCS: {}",
                path
            )));
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "GCS download failed with status {}: {}",
                status, body
            )));
        }

        resp.bytes()
            .await
            .map_err(|e| AppError::Internal(format!("GCS download body read failed: {}", e)))
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let object = self.object_path(path);
        let token = self.get_access_token().await?;

        let url = format!(
            "{}/b/{}/o/{}",
            GCS_API_V1,
            urlencoding::encode(&self.bucket),
            urlencoding::encode(&object),
        );

        let resp = self
            .client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("GCS delete failed: {}", e)))?;

        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "GCS delete failed with status {}: {}",
                status, body
            )));
        }

        Ok(())
    }

    async fn exists(&self, path: &str) -> AppResult<bool> {
        let object = self.object_path(path);
        let token = self.get_access_token().await?;

        let url = format!(
            "{}/b/{}/o/{}",
            GCS_API_V1,
            urlencoding::encode(&self.bucket),
            urlencoding::encode(&object),
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("GCS metadata check failed: {}", e)))?;

        match resp.status() {
            s if s.is_success() => Ok(true),
            reqwest::StatusCode::NOT_FOUND => Ok(false),
            status => Err(AppError::Internal(format!(
                "GCS metadata check failed with status {}",
                status
            ))),
        }
    }

    async fn generate_temp_url(
        &self,
        path: &str,
        expires_in: Duration,
    ) -> AppResult<Option<String>> {
        let object = self.object_path(path);
        let url = self.generate_signed_url_v4(&object, expires_in)?;
        Ok(Some(url))
    }

    async fn list(&self, prefix: &str) -> AppResult<Vec<String>> {
        let full_prefix = self.object_path(prefix);
        let token = self.get_access_token().await?;

        let mut results = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut url = format!(
                "{}/b/{}/o?prefix={}",
                GCS_API_V1,
                urlencoding::encode(&self.bucket),
                urlencoding::encode(&full_prefix),
            );
            if let Some(ref token) = page_token {
                url.push_str(&format!("&pageToken={}", urlencoding::encode(token)));
            }

            let resp = self
                .client
                .get(&url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
                .map_err(|e| AppError::Internal(format!("GCS list failed: {}", e)))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(AppError::Internal(format!(
                    "GCS list failed with status {}: {}",
                    status, body
                )));
            }

            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| AppError::Internal(format!("GCS list parse failed: {}", e)))?;

            if let Some(items) = body["items"].as_array() {
                for item in items {
                    if let Some(name) = item["name"].as_str() {
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

            match body["nextPageToken"].as_str() {
                Some(next) => page_token = Some(next.to_string()),
                None => break,
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Generate a test RSA key for unit tests
    fn generate_test_key() -> (String, RsaPrivateKey) {
        use rsa::pkcs8::EncodePrivateKey;

        let mut rng = rsa::rand_core::OsRng;
        let key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pem = key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap()
            .to_string();
        (pem, key)
    }

    fn test_config() -> GcsConfig {
        let (pem, _) = generate_test_key();
        GcsConfig {
            bucket: "test-bucket".to_string(),
            prefix: "".to_string(),
            client_email: "test@test-project.iam.gserviceaccount.com".to_string(),
            private_key_pem: pem,
            token_uri: Some("http://127.0.0.1:19997/token".to_string()),
        }
    }

    fn test_config_with_prefix() -> GcsConfig {
        GcsConfig {
            prefix: "my-prefix".to_string(),
            ..test_config()
        }
    }

    #[test]
    fn test_object_path_no_prefix() {
        let backend = GcsBackend::new(test_config()).unwrap();
        assert_eq!(backend.object_path("ab/cd/abcdef123"), "ab/cd/abcdef123");
    }

    #[test]
    fn test_object_path_with_prefix() {
        let backend = GcsBackend::new(test_config_with_prefix()).unwrap();
        assert_eq!(
            backend.object_path("ab/cd/abcdef123"),
            "my-prefix/ab/cd/abcdef123"
        );
    }

    #[test]
    fn test_object_path_trailing_slash_prefix() {
        let config = GcsConfig {
            prefix: "my-prefix/".to_string(),
            ..test_config()
        };
        let backend = GcsBackend::new(config).unwrap();
        assert_eq!(backend.object_path("test.txt"), "my-prefix/test.txt");
    }

    #[test]
    fn test_invalid_private_key() {
        let config = GcsConfig {
            private_key_pem: "not-a-valid-pem-key".to_string(),
            ..test_config()
        };
        let result = GcsBackend::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_jwt_creation() {
        let backend = GcsBackend::new(test_config()).unwrap();
        let jwt = backend.create_jwt().unwrap();

        // JWT has three parts separated by dots
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);

        // Decode and verify header
        let header_bytes = BASE64URL.decode(parts[0]).unwrap();
        let header: serde_json::Value = serde_json::from_slice(&header_bytes).unwrap();
        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["typ"], "JWT");

        // Decode and verify payload
        let payload_bytes = BASE64URL.decode(parts[1]).unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&payload_bytes).unwrap();
        assert_eq!(
            payload["iss"],
            "test@test-project.iam.gserviceaccount.com"
        );
        assert!(payload["iat"].is_number());
        assert!(payload["exp"].is_number());
        assert!(payload["scope"].as_str().unwrap().contains("devstorage"));
    }

    #[test]
    fn test_rsa_signing() {
        let backend = GcsBackend::new(test_config()).unwrap();
        let data = b"test data to sign";
        let signature = backend.rsa_sign(data).unwrap();
        assert!(!signature.is_empty());

        // Signature should be deterministic for the same key and data
        let sig2 = backend.rsa_sign(data).unwrap();
        assert_eq!(signature, sig2);
    }

    #[test]
    fn test_signed_url_v4_format() {
        let backend = GcsBackend::new(test_config()).unwrap();
        let url = backend
            .generate_signed_url_v4("path/to/file.txt", Duration::from_secs(3600))
            .unwrap();

        assert!(url.starts_with("https://storage.googleapis.com/"));
        assert!(url.contains("test-bucket"));
        assert!(url.contains("X-Goog-Algorithm=GOOG4-RSA-SHA256"));
        assert!(url.contains("X-Goog-Credential="));
        assert!(url.contains("X-Goog-Date="));
        assert!(url.contains("X-Goog-Expires=3600"));
        assert!(url.contains("X-Goog-SignedHeaders=host"));
        assert!(url.contains("X-Goog-Signature="));
    }

    #[test]
    fn test_signed_url_with_prefix() {
        let backend = GcsBackend::new(test_config_with_prefix()).unwrap();
        let url = backend
            .generate_signed_url_v4("my-prefix/file.txt", Duration::from_secs(300))
            .unwrap();

        assert!(url.contains("my-prefix"));
        assert!(url.contains("X-Goog-Signature="));
    }

    #[test]
    fn test_default_token_uri() {
        let config = GcsConfig {
            token_uri: None,
            ..test_config()
        };
        let backend = GcsBackend::new(config).unwrap();
        assert_eq!(backend.token_uri, TOKEN_URI);
    }

    #[test]
    fn test_config_clone() {
        let config = test_config();
        let cloned = config.clone();
        assert_eq!(cloned.bucket, "test-bucket");
        assert_eq!(cloned.client_email, "test@test-project.iam.gserviceaccount.com");
    }

    #[test]
    fn test_config_debug() {
        let config = test_config();
        let debug = format!("{:?}", config);
        assert!(debug.contains("test-bucket"));
    }

    #[tokio::test]
    async fn test_upload_to_nonexistent_endpoint_returns_error() {
        let config = GcsConfig {
            token_uri: Some("http://127.0.0.1:19997/token".to_string()),
            ..test_config()
        };
        let backend = GcsBackend::new(config).unwrap();

        // This will fail at the token exchange step
        let result = backend.upload("test.txt", Bytes::from("hello")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_download_from_nonexistent_endpoint_returns_error() {
        let config = GcsConfig {
            token_uri: Some("http://127.0.0.1:19997/token".to_string()),
            ..test_config()
        };
        let backend = GcsBackend::new(config).unwrap();

        let result = backend.download("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_from_nonexistent_endpoint_returns_error() {
        let config = GcsConfig {
            token_uri: Some("http://127.0.0.1:19997/token".to_string()),
            ..test_config()
        };
        let backend = GcsBackend::new(config).unwrap();

        let result = backend.delete("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_exists_on_nonexistent_endpoint_returns_error() {
        let config = GcsConfig {
            token_uri: Some("http://127.0.0.1:19997/token".to_string()),
            ..test_config()
        };
        let backend = GcsBackend::new(config).unwrap();

        let result = backend.exists("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_on_nonexistent_endpoint_returns_error() {
        let config = GcsConfig {
            token_uri: Some("http://127.0.0.1:19997/token".to_string()),
            ..test_config()
        };
        let backend = GcsBackend::new(config).unwrap();

        let result = backend.list("").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_generate_temp_url_returns_signed_url() {
        let backend = GcsBackend::new(test_config()).unwrap();
        let result = backend
            .generate_temp_url("test-file.txt", Duration::from_secs(3600))
            .await;

        assert!(result.is_ok());
        let url = result.unwrap();
        assert!(url.is_some());
        let url_str = url.unwrap();
        assert!(url_str.contains("test-file.txt"));
        assert!(url_str.contains("X-Goog-Signature="));
    }
}
