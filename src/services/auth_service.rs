use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::AuthConfig;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub role: String,
    pub exp: usize,
}

#[derive(Clone)]
pub struct AuthService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    access_token_ttl_secs: u64,
    pub refresh_token_ttl_secs: u64,
}

impl AuthService {
    pub fn new(config: &AuthConfig) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(config.jwt_secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(config.jwt_secret.as_bytes()),
            access_token_ttl_secs: config.access_token_ttl_secs,
            refresh_token_ttl_secs: config.refresh_token_ttl_secs,
        }
    }

    pub fn hash_password(&self, password: &str) -> Result<String, argon2::password_hash::Error> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let hash = argon2.hash_password(password.as_bytes(), &salt)?;
        Ok(hash.to_string())
    }

    pub fn verify_password(&self, password: &str, hash: &str) -> bool {
        let parsed_hash = match PasswordHash::new(hash) {
            Ok(h) => h,
            Err(_) => return false,
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok()
    }

    pub fn generate_access_token(
        &self,
        user_id: Uuid,
        role: &str,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let now = chrono::Utc::now().timestamp() as usize;
        let claims = Claims {
            sub: user_id.to_string(),
            role: role.to_string(),
            exp: now + self.access_token_ttl_secs as usize,
        };
        encode(&Header::default(), &claims, &self.encoding_key)
    }

    pub fn generate_refresh_token(&self) -> String {
        let random_bytes: [u8; 32] = rand::thread_rng().gen();
        hex::encode(random_bytes)
    }

    pub fn validate_access_token(
        &self,
        token: &str,
    ) -> Result<Claims, jsonwebtoken::errors::Error> {
        let token_data = decode::<Claims>(token, &self.decoding_key, &Validation::default())?;
        Ok(token_data.claims)
    }

    pub fn hash_refresh_token(token: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthConfig;

    fn test_config() -> AuthConfig {
        AuthConfig {
            jwt_secret: "test-secret-key-for-testing".to_string(),
            access_token_ttl_secs: 900,
            refresh_token_ttl_secs: 604800,
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin123".to_string(),
        }
    }

    #[test]
    fn test_hash_and_verify_password() {
        let service = AuthService::new(&test_config());
        let password = "my-secure-password-123";
        let hash = service.hash_password(password).unwrap();

        assert!(service.verify_password(password, &hash));
        assert!(!service.verify_password("wrong-password", &hash));
    }

    #[test]
    fn test_hash_password_produces_different_hashes() {
        let service = AuthService::new(&test_config());
        let password = "same-password";
        let hash1 = service.hash_password(password).unwrap();
        let hash2 = service.hash_password(password).unwrap();

        // Different salts should produce different hashes
        assert_ne!(hash1, hash2);
        // But both should verify
        assert!(service.verify_password(password, &hash1));
        assert!(service.verify_password(password, &hash2));
    }

    #[test]
    fn test_verify_password_with_invalid_hash() {
        let service = AuthService::new(&test_config());
        assert!(!service.verify_password("password", "not-a-valid-hash"));
    }

    #[test]
    fn test_generate_and_validate_access_token() {
        let service = AuthService::new(&test_config());
        let user_id = Uuid::new_v4();
        let role = "admin";

        let token = service.generate_access_token(user_id, role).unwrap();
        let claims = service.validate_access_token(&token).unwrap();

        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.role, "admin");
    }

    #[test]
    fn test_expired_token_rejected() {
        let config = AuthConfig {
            jwt_secret: "test-secret-key-for-testing".to_string(),
            access_token_ttl_secs: 0, // expires immediately
            refresh_token_ttl_secs: 604800,
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin123".to_string(),
        };
        let service = AuthService::new(&config);
        let user_id = Uuid::new_v4();

        let token = service.generate_access_token(user_id, "user").unwrap();

        // Token with 0 TTL should be expired
        let result = service.validate_access_token(&token);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_token_rejected() {
        let service = AuthService::new(&test_config());
        let result = service.validate_access_token("not.a.valid.token");
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_secret_rejected() {
        let service1 = AuthService::new(&test_config());
        let service2 = AuthService::new(&AuthConfig {
            jwt_secret: "different-secret-key".to_string(),
            access_token_ttl_secs: 900,
            refresh_token_ttl_secs: 604800,
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin123".to_string(),
        });

        let user_id = Uuid::new_v4();
        let token = service1.generate_access_token(user_id, "user").unwrap();

        let result = service2.validate_access_token(&token);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_refresh_token() {
        let service = AuthService::new(&test_config());
        let token1 = service.generate_refresh_token();
        let token2 = service.generate_refresh_token();

        // Should be 64 hex chars (32 bytes)
        assert_eq!(token1.len(), 64);
        assert_eq!(token2.len(), 64);
        // Should be different each time
        assert_ne!(token1, token2);
    }

    #[test]
    fn test_hash_refresh_token() {
        let token = "abc123";
        let hash1 = AuthService::hash_refresh_token(token);
        let hash2 = AuthService::hash_refresh_token(token);

        // Same input should produce same hash (SHA-256 is deterministic)
        assert_eq!(hash1, hash2);
        // Different input should produce different hash
        assert_ne!(hash1, AuthService::hash_refresh_token("different"));
    }

    #[test]
    fn test_claims_serialization() {
        let claims = Claims {
            sub: Uuid::new_v4().to_string(),
            role: "admin".to_string(),
            exp: 1234567890,
        };

        let json = serde_json::to_string(&claims).unwrap();
        let deserialized: Claims = serde_json::from_str(&json).unwrap();

        assert_eq!(claims.sub, deserialized.sub);
        assert_eq!(claims.role, deserialized.role);
        assert_eq!(claims.exp, deserialized.exp);
    }
}
