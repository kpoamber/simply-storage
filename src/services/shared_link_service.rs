use bytes::Bytes;
use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::models::{
    CreateSharedLink, File, FileLocation, FileReference, ProjectStorage, SharedLink, Storage,
};
use crate::error::{AppError, AppResult};
use crate::storage::registry::create_backend;
use crate::storage::traits::StorageBackend;
use crate::storage::StorageRegistry;

/// Public information about a shared link (safe to expose to anonymous users).
#[derive(Debug, Serialize)]
pub struct SharedLinkInfo {
    pub file_name: String,
    pub file_size: i64,
    pub content_type: String,
    pub password_required: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Result of a successful download via shared link.
#[derive(Debug)]
pub struct SharedLinkDownloadResult {
    pub data: Bytes,
    pub content_type: String,
    pub file_name: String,
}

/// Claims for short-lived download tokens (password-protected links).
#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadTokenClaims {
    pub sub: String,
    pub link_id: String,
    pub exp: usize,
}

/// Input for creating a shared link via the service layer.
#[derive(Debug, Deserialize)]
pub struct CreateSharedLinkInput {
    pub file_id: Uuid,
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub user_role: String,
    pub password: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_downloads: Option<i32>,
}

const DOWNLOAD_TOKEN_TTL_SECS: usize = 300; // 5 minutes

/// Check that a shared link is still accessible (active and not expired).
/// Free function for testability.
pub(crate) fn validate_link_accessible(link: &SharedLink) -> AppResult<()> {
    if !link.is_active {
        return Err(AppError::NotFound("Shared link not found".to_string()));
    }
    if let Some(expires_at) = link.expires_at {
        if Utc::now() > expires_at {
            return Err(AppError::NotFound("Shared link not found".to_string()));
        }
    }
    Ok(())
}

pub struct SharedLinkService {
    pool: PgPool,
    registry: Arc<StorageRegistry>,
    hmac_secret: String,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl SharedLinkService {
    pub fn new(pool: PgPool, registry: Arc<StorageRegistry>, jwt_secret: &str, hmac_secret: String) -> Self {
        Self {
            pool,
            registry,
            hmac_secret,
            encoding_key: EncodingKey::from_secret(format!("dl-token:{}", jwt_secret).as_bytes()),
            decoding_key: DecodingKey::from_secret(format!("dl-token:{}", jwt_secret).as_bytes()),
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Create a shared link for a file. Validates file exists and belongs to the project.
    /// Note: project access authorization is handled by the API handler (check_project_access).
    pub async fn create_link(&self, input: &CreateSharedLinkInput) -> AppResult<SharedLink> {
        // Validate file exists
        let file = File::find_by_id(&self.pool, input.file_id).await?;

        // Validate file has a reference in this project
        let refs = FileReference::find_by_file_and_project(
            &self.pool,
            input.file_id,
            input.project_id,
        )
        .await?;
        if refs.is_empty() {
            return Err(AppError::NotFound(
                "File not found in this project".to_string(),
            ));
        }

        let original_name = refs[0].original_name.clone();

        let create = CreateSharedLink {
            file_id: file.id,
            project_id: input.project_id,
            original_name,
            created_by: input.user_id,
            password: input.password.clone(),
            expires_at: input.expires_at,
            max_downloads: input.max_downloads,
        };

        SharedLink::create(&self.pool, &create).await
    }

    /// Get public info about a shared link (no auth required).
    pub async fn get_link_info(&self, token: &str) -> AppResult<SharedLinkInfo> {
        let link = SharedLink::find_by_token(&self.pool, token).await?;

        validate_link_accessible(&link)?;

        let file = File::find_by_id(&self.pool, link.file_id).await?;

        Ok(SharedLinkInfo {
            file_name: link.original_name,
            file_size: file.size,
            content_type: file.content_type,
            password_required: link.password_hash.is_some(),
            expires_at: link.expires_at,
        })
    }

    /// Verify password for a protected link. Returns a short-lived download token on success.
    pub async fn verify_password(&self, token: &str, password: &str) -> AppResult<String> {
        let link = SharedLink::find_by_token(&self.pool, token).await?;

        validate_link_accessible(&link)?;

        let hash = link.password_hash.as_ref().ok_or_else(|| {
            AppError::BadRequest("This link is not password-protected".to_string())
        })?;

        if !SharedLink::verify_password(password, hash) {
            return Err(AppError::Forbidden("Wrong password".to_string()));
        }

        self.generate_download_token(&link.token, link.id)
    }

    /// Download file via shared link. For password-protected links, requires a valid download token.
    pub async fn download_via_link(
        &self,
        token: &str,
        dl_token: Option<&str>,
    ) -> AppResult<SharedLinkDownloadResult> {
        let link = SharedLink::find_by_token(&self.pool, token).await?;

        validate_link_accessible(&link)?;

        // For password-protected links, verify the download token
        if link.password_hash.is_some() {
            let dl_token = dl_token.ok_or_else(|| {
                AppError::Unauthorized(
                    "Download token required for password-protected links".to_string(),
                )
            })?;
            self.validate_download_token(dl_token, &link.token, link.id)?;
        }

        // Check max_downloads limit before attempting download.
        // We do NOT increment here — increment happens after successful download
        // to avoid consuming quota on transient storage failures.
        if let Some(max) = link.max_downloads {
            if link.download_count >= max as i64 {
                return Err(AppError::Forbidden("Download limit reached".to_string()));
            }
        }

        // Download from storage
        let file = File::find_by_id(&self.pool, link.file_id).await?;
        let locations = FileLocation::find_for_file(&self.pool, link.file_id).await?;
        if locations.is_empty() {
            return Err(AppError::NotFound(
                "File not available for download".to_string(),
            ));
        }

        let refs = FileReference::find_by_file_id(&self.pool, link.file_id).await?;

        for location in &locations {
            let backends = self
                .resolve_backends_for_location(&location.storage_id, &refs)
                .await;

            for backend in &backends {
                match backend.download(&location.storage_path).await {
                    Ok(data) => {
                        let _ = FileLocation::touch_accessed(&self.pool, location.id).await;
                        // Atomically increment download count AFTER successful download.
                        // The atomic SQL WHERE clause prevents exceeding max_downloads
                        // even under concurrent requests.
                        let incremented =
                            SharedLink::increment_download_count(&self.pool, link.id).await?;
                        if !incremented {
                            return Err(AppError::Forbidden("Download limit reached".to_string()));
                        }
                        return Ok(SharedLinkDownloadResult {
                            data,
                            content_type: file.content_type.clone(),
                            file_name: link.original_name,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            storage_id = %location.storage_id,
                            file_id = %link.file_id,
                            error = %e,
                            "Shared link download: storage location failed, trying next"
                        );
                    }
                }
            }
        }

        Err(AppError::Internal(format!(
            "All storage locations failed for file {}",
            link.file_id
        )))
    }

    /// List shared links for a project.
    pub async fn list_links(&self, project_id: Uuid) -> AppResult<Vec<SharedLink>> {
        SharedLink::list_by_project(&self.pool, project_id).await
    }

    /// Deactivate a shared link. Only the creator or an admin can do this.
    pub async fn deactivate_link(
        &self,
        link_id: Uuid,
        user_id: Uuid,
        user_role: &str,
    ) -> AppResult<SharedLink> {
        let link = SharedLink::find_by_id(&self.pool, link_id).await?;

        if user_role != "admin" && link.created_by != user_id {
            return Err(AppError::Forbidden(
                "Only the link creator or an admin can deactivate this link".to_string(),
            ));
        }

        SharedLink::deactivate(&self.pool, link_id).await
    }

    // ─── Private helpers ────────────────────────────────────────────────────

    /// Generate a short-lived JWT download token for password-protected links.
    fn generate_download_token(&self, token: &str, link_id: Uuid) -> AppResult<String> {
        let now = chrono::Utc::now().timestamp() as usize;
        let claims = DownloadTokenClaims {
            sub: token.to_string(),
            link_id: link_id.to_string(),
            exp: now + DOWNLOAD_TOKEN_TTL_SECS,
        };
        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| AppError::Internal(format!("Failed to generate download token: {}", e)))
    }

    /// Validate a download token for a specific shared link.
    fn validate_download_token(
        &self,
        dl_token: &str,
        expected_token: &str,
        expected_link_id: Uuid,
    ) -> AppResult<()> {
        let token_data =
            decode::<DownloadTokenClaims>(dl_token, &self.decoding_key, &Validation::default())
                .map_err(|_| {
                    AppError::Unauthorized("Invalid or expired download token".to_string())
                })?;

        if token_data.claims.sub != expected_token
            || token_data.claims.link_id != expected_link_id.to_string()
        {
            return Err(AppError::Unauthorized(
                "Download token does not match this link".to_string(),
            ));
        }

        Ok(())
    }

    /// Resolve all possible backends for a storage location, considering container overrides.
    async fn resolve_backends_for_location(
        &self,
        storage_id: &Uuid,
        file_refs: &[FileReference],
    ) -> Vec<Arc<dyn StorageBackend>> {
        let mut backends: Vec<Arc<dyn StorageBackend>> = Vec::new();

        if let Ok(storage) = Storage::find_by_id(&self.pool, *storage_id).await {
            for fref in file_refs {
                if let Ok(backend) = self.get_project_backend(&storage, fref.project_id).await {
                    backends.push(backend);
                }
            }
        }

        // Fallback: default backend from registry
        if let Ok(default_backend) = self.registry.get(storage_id).await {
            backends.push(default_backend);
        }

        backends
    }

    /// Get storage backend with project-specific container/prefix overrides.
    async fn get_project_backend(
        &self,
        storage: &Storage,
        project_id: Uuid,
    ) -> AppResult<Arc<dyn StorageBackend>> {
        let assignment =
            ProjectStorage::find_for_project_and_storage(&self.pool, project_id, storage.id)
                .await?;

        if let Some(ps) = assignment {
            if ps.container_override.is_some() || ps.prefix_override.is_some() {
                let mut config = storage.config.clone();
                if let Some(ref container) = ps.container_override {
                    match storage.storage_type.as_str() {
                        "s3" | "gcs" => {
                            config["bucket"] = serde_json::Value::String(container.clone());
                        }
                        "azure" => {
                            config["container"] = serde_json::Value::String(container.clone());
                        }
                        _ => {}
                    }
                }
                if let Some(ref prefix) = ps.prefix_override {
                    config["prefix"] = serde_json::Value::String(prefix.clone());
                }
                return create_backend(&storage.storage_type, &config, &self.hmac_secret).await;
            }
        }

        self.registry.get(&storage.id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_token_generation_and_validation() {
        let secret = "test-jwt-secret-for-shared-links";
        let pool = {
            // We only need the encoding/decoding keys for this test, not a real pool.
            // Use a dummy runtime-less approach via direct JWT calls.
            let encoding_key = EncodingKey::from_secret(secret.as_bytes());
            let decoding_key = DecodingKey::from_secret(secret.as_bytes());

            let token = "abc123token";
            let link_id = Uuid::new_v4();
            let now = chrono::Utc::now().timestamp() as usize;
            let claims = DownloadTokenClaims {
                sub: token.to_string(),
                link_id: link_id.to_string(),
                exp: now + DOWNLOAD_TOKEN_TTL_SECS,
            };

            let jwt = encode(&Header::default(), &claims, &encoding_key).unwrap();
            let decoded =
                decode::<DownloadTokenClaims>(&jwt, &decoding_key, &Validation::default())
                    .unwrap();

            assert_eq!(decoded.claims.sub, token);
            assert_eq!(decoded.claims.link_id, link_id.to_string());

            // Wrong token should not match
            assert_ne!(decoded.claims.sub, "wrong-token");
        };
        let _ = pool;
    }

    #[test]
    fn test_download_token_expired() {
        let secret = "test-jwt-secret-for-shared-links";
        let encoding_key = EncodingKey::from_secret(secret.as_bytes());
        let decoding_key = DecodingKey::from_secret(secret.as_bytes());

        let claims = DownloadTokenClaims {
            sub: "test-token".to_string(),
            link_id: Uuid::new_v4().to_string(),
            exp: 0, // expired
        };

        let jwt = encode(&Header::default(), &claims, &encoding_key).unwrap();
        let result = decode::<DownloadTokenClaims>(&jwt, &decoding_key, &Validation::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_download_token_wrong_secret() {
        let encoding_key = EncodingKey::from_secret(b"secret-one");
        let decoding_key = DecodingKey::from_secret(b"secret-two");

        let now = chrono::Utc::now().timestamp() as usize;
        let claims = DownloadTokenClaims {
            sub: "test-token".to_string(),
            link_id: Uuid::new_v4().to_string(),
            exp: now + 300,
        };

        let jwt = encode(&Header::default(), &claims, &encoding_key).unwrap();
        let result = decode::<DownloadTokenClaims>(&jwt, &decoding_key, &Validation::default());
        assert!(result.is_err());
    }

    fn make_test_link() -> SharedLink {
        SharedLink {
            id: Uuid::new_v4(),
            token: "test".to_string(),
            file_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            original_name: "test.txt".to_string(),
            created_by: Uuid::new_v4(),
            password_hash: None,
            expires_at: None,
            max_downloads: None,
            download_count: 0,
            last_accessed_at: None,
            is_active: true,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_validate_link_accessible_active_no_expiry() {
        let link = make_test_link();
        assert!(validate_link_accessible(&link).is_ok());
    }

    #[test]
    fn test_validate_link_inactive_returns_not_found() {
        let mut link = make_test_link();
        link.is_active = false;
        let err = validate_link_accessible(&link).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn test_validate_link_expired_returns_not_found() {
        let mut link = make_test_link();
        link.expires_at = Some(Utc::now() - chrono::Duration::hours(1));
        let err = validate_link_accessible(&link).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn test_validate_link_not_expired_is_ok() {
        let mut link = make_test_link();
        link.expires_at = Some(Utc::now() + chrono::Duration::hours(1));
        assert!(validate_link_accessible(&link).is_ok());
    }

    #[test]
    fn test_validate_link_inactive_and_expired() {
        let mut link = make_test_link();
        link.is_active = false;
        link.expires_at = Some(Utc::now() - chrono::Duration::hours(1));
        // Inactive check comes first
        let err = validate_link_accessible(&link).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn test_password_protected_helper() {
        let public_link = make_test_link();
        assert!(!public_link.password_protected());

        let mut protected_link = make_test_link();
        protected_link.password_hash = Some("$argon2id$v=19$...".to_string());
        assert!(protected_link.password_protected());
    }

    #[test]
    fn test_shared_link_info_serialization() {
        let info = SharedLinkInfo {
            file_name: "report.pdf".to_string(),
            file_size: 1024000,
            content_type: "application/pdf".to_string(),
            password_required: true,
            expires_at: Some(Utc::now() + chrono::Duration::hours(24)),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("report.pdf"));
        assert!(json.contains("1024000"));
        assert!(json.contains("application/pdf"));
        assert!(json.contains("\"password_required\":true"));
    }

    #[test]
    fn test_download_token_claims_serialization() {
        let claims = DownloadTokenClaims {
            sub: "abc123".to_string(),
            link_id: Uuid::new_v4().to_string(),
            exp: 1234567890,
        };

        let json = serde_json::to_string(&claims).unwrap();
        let deserialized: DownloadTokenClaims = serde_json::from_str(&json).unwrap();
        assert_eq!(claims.sub, deserialized.sub);
        assert_eq!(claims.link_id, deserialized.link_id);
        assert_eq!(claims.exp, deserialized.exp);
    }
}
