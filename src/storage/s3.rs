use async_trait::async_trait;
use aws_config::Region;
use aws_credential_types::Credentials;
use aws_sdk_s3::{
    config::{BehaviorVersion, Builder as S3ConfigBuilder},
    presigning::PresigningConfig,
    primitives::ByteStream,
    Client,
};
use bytes::Bytes;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::storage::traits::StorageBackend;

/// Default multipart upload threshold: 100 MB.
const DEFAULT_MULTIPART_THRESHOLD: u64 = 100 * 1024 * 1024;

/// Default part size for multipart uploads: 10 MB.
const DEFAULT_PART_SIZE: u64 = 10 * 1024 * 1024;

/// S3-compatible storage backend.
///
/// Works with AWS S3, DigitalOcean Spaces, MinIO, LocalStack, and any other
/// S3-compatible service by setting a custom `endpoint_url`.
pub struct S3StorageBackend {
    client: Client,
    bucket: String,
    prefix: String,
    multipart_threshold: u64,
    part_size: u64,
}

/// Configuration for creating an S3 storage backend.
#[derive(Debug, Clone)]
pub struct S3Config {
    pub endpoint_url: Option<String>,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub multipart_threshold: Option<u64>,
    pub part_size: Option<u64>,
    pub force_path_style: bool,
}

impl S3StorageBackend {
    /// Create a new S3 storage backend from the given configuration.
    pub async fn new(config: S3Config) -> Self {
        let credentials = Credentials::new(
            &config.access_key_id,
            &config.secret_access_key,
            None,
            None,
            "innovare-storage",
        );

        let mut s3_config_builder = S3ConfigBuilder::new()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(config.region))
            .credentials_provider(credentials)
            .force_path_style(config.force_path_style);

        if let Some(endpoint) = &config.endpoint_url {
            s3_config_builder = s3_config_builder.endpoint_url(endpoint);
        }

        let client = Client::from_conf(s3_config_builder.build());

        Self {
            client,
            bucket: config.bucket,
            prefix: config.prefix,
            multipart_threshold: config.multipart_threshold.unwrap_or(DEFAULT_MULTIPART_THRESHOLD),
            part_size: config.part_size.unwrap_or(DEFAULT_PART_SIZE),
        }
    }

    /// Build the full S3 object key from a logical storage path.
    fn object_key(&self, path: &str) -> String {
        if self.prefix.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", self.prefix.trim_end_matches('/'), path)
        }
    }

    /// Upload a file using multipart upload for large files.
    async fn multipart_upload(&self, key: &str, data: &Bytes) -> AppResult<()> {
        let create_resp = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("S3 create multipart upload failed: {}", e)))?;

        let upload_id = create_resp
            .upload_id()
            .ok_or_else(|| AppError::Internal("S3 multipart upload: no upload_id returned".into()))?
            .to_string();

        let mut completed_parts = Vec::new();
        let total_size = data.len() as u64;
        let part_size = self.part_size as usize;
        let mut offset = 0usize;
        let mut part_number = 1i32;

        while offset < data.len() {
            let end = std::cmp::min(offset + part_size, data.len());
            let part_data = data.slice(offset..end);

            let upload_result = self
                .client
                .upload_part()
                .bucket(&self.bucket)
                .key(key)
                .upload_id(&upload_id)
                .part_number(part_number)
                .body(ByteStream::from(part_data))
                .content_length(((end - offset) as u64).try_into().unwrap_or(0))
                .send()
                .await;

            match upload_result {
                Ok(resp) => {
                    let etag = resp.e_tag().unwrap_or_default().to_string();
                    completed_parts.push(
                        aws_sdk_s3::types::CompletedPart::builder()
                            .part_number(part_number)
                            .e_tag(etag)
                            .build(),
                    );
                }
                Err(e) => {
                    // Abort the multipart upload on failure
                    let _ = self
                        .client
                        .abort_multipart_upload()
                        .bucket(&self.bucket)
                        .key(key)
                        .upload_id(&upload_id)
                        .send()
                        .await;
                    return Err(AppError::Internal(format!(
                        "S3 upload part {} failed ({}B of {}B uploaded): {}",
                        part_number, offset, total_size, e
                    )));
                }
            }

            offset = end;
            part_number += 1;
        }

        let completed_upload = aws_sdk_s3::types::CompletedMultipartUpload::builder()
            .set_parts(Some(completed_parts))
            .build();

        self.client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(&upload_id)
            .multipart_upload(completed_upload)
            .send()
            .await
            .map_err(|e| {
                AppError::Internal(format!("S3 complete multipart upload failed: {}", e))
            })?;

        Ok(())
    }
}

#[async_trait]
impl StorageBackend for S3StorageBackend {
    async fn upload(&self, path: &str, data: Bytes) -> AppResult<()> {
        let key = self.object_key(path);

        if (data.len() as u64) >= self.multipart_threshold {
            return self.multipart_upload(&key, &data).await;
        }

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(ByteStream::from(data))
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("S3 upload failed: {}", e)))?;

        Ok(())
    }

    async fn download(&self, path: &str) -> AppResult<Bytes> {
        let key = self.object_key(path);

        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| {
                let err_str = format!("{}", e);
                if err_str.contains("NoSuchKey") || err_str.contains("404") {
                    AppError::NotFound(format!("File not found in S3: {}", path))
                } else {
                    AppError::Internal(format!("S3 download failed: {}", e))
                }
            })?;

        let body = resp.body.collect().await.map_err(|e| {
            AppError::Internal(format!("S3 download body read failed: {}", e))
        })?;

        Ok(body.into_bytes())
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let key = self.object_key(path);

        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("S3 delete failed: {}", e)))?;

        Ok(())
    }

    async fn exists(&self, path: &str) -> AppResult<bool> {
        let key = self.object_key(path);

        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let err_str = format!("{}", e);
                if err_str.contains("NotFound") || err_str.contains("404") || err_str.contains("NoSuchKey") {
                    Ok(false)
                } else {
                    Err(AppError::Internal(format!("S3 head object failed: {}", e)))
                }
            }
        }
    }

    async fn generate_temp_url(
        &self,
        path: &str,
        expires_in: Duration,
    ) -> AppResult<Option<String>> {
        let key = self.object_key(path);

        let presigning_config = PresigningConfig::builder()
            .expires_in(expires_in)
            .build()
            .map_err(|e| AppError::Internal(format!("S3 presigning config error: {}", e)))?;

        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .presigned(presigning_config)
            .await
            .map_err(|e| AppError::Internal(format!("S3 presigned URL generation failed: {}", e)))?;

        Ok(Some(presigned.uri().to_string()))
    }

    async fn list(&self, prefix: &str) -> AppResult<Vec<String>> {
        let full_prefix = self.object_key(prefix);
        let mut results = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&full_prefix);

            if let Some(token) = &continuation_token {
                request = request.continuation_token(token);
            }

            let resp = request
                .send()
                .await
                .map_err(|e| AppError::Internal(format!("S3 list objects failed: {}", e)))?;

            for object in resp.contents() {
                if let Some(key) = object.key() {
                    // Strip the prefix to return relative paths
                    let relative = if !self.prefix.is_empty() {
                        let prefix_with_slash =
                            format!("{}/", self.prefix.trim_end_matches('/'));
                        key.strip_prefix(&prefix_with_slash)
                            .unwrap_or(key)
                            .to_string()
                    } else {
                        key.to_string()
                    };
                    results.push(relative);
                }
            }

            if resp.is_truncated() == Some(true) {
                continuation_token = resp.next_continuation_token().map(|s| s.to_string());
            } else {
                break;
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> S3Config {
        S3Config {
            endpoint_url: Some("http://localhost:4566".to_string()),
            region: "us-east-1".to_string(),
            bucket: "test-bucket".to_string(),
            prefix: "".to_string(),
            access_key_id: "test".to_string(),
            secret_access_key: "test".to_string(),
            multipart_threshold: None,
            part_size: None,
            force_path_style: true,
        }
    }

    fn test_config_with_prefix() -> S3Config {
        S3Config {
            prefix: "my-prefix".to_string(),
            ..test_config()
        }
    }

    #[test]
    fn test_object_key_no_prefix() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let backend = rt.block_on(S3StorageBackend::new(test_config()));

        assert_eq!(backend.object_key("abcdef123"), "abcdef123");
        assert_eq!(backend.object_key("path/to/file"), "path/to/file");
    }

    #[test]
    fn test_object_key_with_prefix() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let backend = rt.block_on(S3StorageBackend::new(test_config_with_prefix()));

        assert_eq!(backend.object_key("abcdef123"), "my-prefix/abcdef123");
        assert_eq!(
            backend.object_key("path/to/file"),
            "my-prefix/path/to/file"
        );
    }

    #[test]
    fn test_object_key_with_trailing_slash_prefix() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = S3Config {
            prefix: "my-prefix/".to_string(),
            ..test_config()
        };
        let backend = rt.block_on(S3StorageBackend::new(config));

        assert_eq!(backend.object_key("abcdef123"), "my-prefix/abcdef123");
    }

    #[test]
    fn test_default_multipart_threshold() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let backend = rt.block_on(S3StorageBackend::new(test_config()));

        assert_eq!(backend.multipart_threshold, DEFAULT_MULTIPART_THRESHOLD);
        assert_eq!(backend.part_size, DEFAULT_PART_SIZE);
    }

    #[test]
    fn test_custom_multipart_threshold() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = S3Config {
            multipart_threshold: Some(50 * 1024 * 1024),
            part_size: Some(5 * 1024 * 1024),
            ..test_config()
        };
        let backend = rt.block_on(S3StorageBackend::new(config));

        assert_eq!(backend.multipart_threshold, 50 * 1024 * 1024);
        assert_eq!(backend.part_size, 5 * 1024 * 1024);
    }

    #[test]
    fn test_s3_config_clone() {
        let config = test_config();
        let cloned = config.clone();
        assert_eq!(cloned.bucket, "test-bucket");
        assert_eq!(cloned.region, "us-east-1");
        assert_eq!(cloned.endpoint_url, Some("http://localhost:4566".to_string()));
        assert!(cloned.force_path_style);
    }

    #[test]
    fn test_s3_config_debug() {
        let config = test_config();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("test-bucket"));
        assert!(debug_str.contains("us-east-1"));
    }

    #[test]
    fn test_do_spaces_config() {
        // DigitalOcean Spaces uses S3-compatible API with custom endpoint
        let config = S3Config {
            endpoint_url: Some("https://ams3.digitaloceanspaces.com".to_string()),
            region: "ams3".to_string(),
            bucket: "my-space".to_string(),
            prefix: "files".to_string(),
            access_key_id: "DO_ACCESS_KEY".to_string(),
            secret_access_key: "DO_SECRET_KEY".to_string(),
            multipart_threshold: None,
            part_size: None,
            force_path_style: false,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let backend = rt.block_on(S3StorageBackend::new(config));

        assert_eq!(backend.bucket, "my-space");
        assert_eq!(backend.object_key("test.txt"), "files/test.txt");
    }

    #[tokio::test]
    async fn test_presigned_url_generation() {
        // This test verifies that presigned URL generation doesn't panic
        // and returns a valid URL structure. The actual URL won't work
        // without a real S3 endpoint, but the signing logic is exercised.
        let backend = S3StorageBackend::new(test_config()).await;

        let result = backend
            .generate_temp_url("test-file.txt", Duration::from_secs(3600))
            .await;

        // The presigned URL generation should succeed even without a real endpoint
        assert!(result.is_ok());
        let url = result.unwrap();
        assert!(url.is_some());
        let url_str = url.unwrap();
        // Presigned URLs contain the signature and expiry parameters
        assert!(url_str.contains("test-file.txt"));
        assert!(url_str.contains("X-Amz-"));
    }

    #[tokio::test]
    async fn test_presigned_url_with_prefix() {
        let backend = S3StorageBackend::new(test_config_with_prefix()).await;

        let result = backend
            .generate_temp_url("test-file.txt", Duration::from_secs(300))
            .await;

        assert!(result.is_ok());
        let url = result.unwrap();
        assert!(url.is_some());
        let url_str = url.unwrap();
        assert!(url_str.contains("my-prefix"));
        assert!(url_str.contains("test-file.txt"));
    }

    #[tokio::test]
    async fn test_upload_to_nonexistent_endpoint_returns_error() {
        // Verifying error handling when the S3 endpoint is unreachable
        let config = S3Config {
            endpoint_url: Some("http://127.0.0.1:19999".to_string()),
            ..test_config()
        };
        let backend = S3StorageBackend::new(config).await;

        let result = backend
            .upload("test.txt", Bytes::from("hello"))
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::Internal(msg) => assert!(msg.contains("S3 upload failed")),
            other => panic!("Expected Internal error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_download_from_nonexistent_endpoint_returns_error() {
        let config = S3Config {
            endpoint_url: Some("http://127.0.0.1:19999".to_string()),
            ..test_config()
        };
        let backend = S3StorageBackend::new(config).await;

        let result = backend.download("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_from_nonexistent_endpoint_returns_error() {
        let config = S3Config {
            endpoint_url: Some("http://127.0.0.1:19999".to_string()),
            ..test_config()
        };
        let backend = S3StorageBackend::new(config).await;

        let result = backend.delete("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_exists_on_nonexistent_endpoint_returns_error() {
        let config = S3Config {
            endpoint_url: Some("http://127.0.0.1:19999".to_string()),
            ..test_config()
        };
        let backend = S3StorageBackend::new(config).await;

        let result = backend.exists("test.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_on_nonexistent_endpoint_returns_error() {
        let config = S3Config {
            endpoint_url: Some("http://127.0.0.1:19999".to_string()),
            ..test_config()
        };
        let backend = S3StorageBackend::new(config).await;

        let result = backend.list("").await;
        assert!(result.is_err());
    }
}
