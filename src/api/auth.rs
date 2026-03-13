use actix_web::{FromRequest, HttpRequest, dev::Payload, web};
use std::future::{Ready, ready};
use uuid::Uuid;

use crate::error::AppError;
use crate::services::auth_service::AuthService;

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: Uuid,
    pub role: String,
}

impl AuthenticatedUser {
    pub fn require_admin(&self) -> Result<(), AppError> {
        if self.role == "admin" {
            Ok(())
        } else {
            Err(AppError::Forbidden(
                "Admin access required".to_string(),
            ))
        }
    }

    pub fn require_owner_or_admin(&self, owner_id: Option<Uuid>) -> Result<(), AppError> {
        if self.role == "admin" {
            return Ok(());
        }
        match owner_id {
            Some(oid) if oid == self.user_id => Ok(()),
            _ => Err(AppError::Forbidden(
                "Access denied: not the owner".to_string(),
            )),
        }
    }
}

impl FromRequest for AuthenticatedUser {
    type Error = AppError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        ready(extract_user(req))
    }
}

fn extract_user(req: &HttpRequest) -> Result<AuthenticatedUser, AppError> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("Missing Authorization header".to_string()))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| {
            AppError::Unauthorized("Invalid Authorization header format, expected: Bearer <token>".to_string())
        })?;

    let auth_service = req
        .app_data::<web::Data<AuthService>>()
        .ok_or_else(|| AppError::Internal("AuthService not configured".to_string()))?;

    let claims = auth_service
        .validate_access_token(token)
        .map_err(|e| AppError::Unauthorized(format!("Invalid token: {}", e)))?;

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Unauthorized("Invalid user ID in token".to_string()))?;

    Ok(AuthenticatedUser {
        user_id,
        role: claims.role,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, web, App, HttpResponse};
    use crate::config::AuthConfig;

    fn test_auth_service() -> AuthService {
        AuthService::new(&AuthConfig {
            jwt_secret: "test-secret-for-auth-extractor".to_string(),
            access_token_ttl_secs: 900,
            refresh_token_ttl_secs: 604800,
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin123".to_string(),
        })
    }

    async fn protected_endpoint(user: AuthenticatedUser) -> HttpResponse {
        HttpResponse::Ok().json(serde_json::json!({
            "user_id": user.user_id.to_string(),
            "role": user.role,
        }))
    }

    #[actix_rt::test]
    async fn test_extract_valid_token() {
        let auth_service = test_auth_service();
        let user_id = Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "admin").unwrap();

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(auth_service))
                .route("/protected", web::get().to(protected_endpoint)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/protected")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 200);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["user_id"], user_id.to_string());
        assert_eq!(body["role"], "admin");
    }

    #[actix_rt::test]
    async fn test_reject_missing_token() {
        let auth_service = test_auth_service();

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(auth_service))
                .route("/protected", web::get().to(protected_endpoint)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/protected")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_reject_invalid_format() {
        let auth_service = test_auth_service();

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(auth_service))
                .route("/protected", web::get().to(protected_endpoint)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/protected")
            .insert_header(("Authorization", "Basic dXNlcjpwYXNz"))
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_reject_expired_token() {
        let auth_service = AuthService::new(&AuthConfig {
            jwt_secret: "test-secret-for-auth-extractor".to_string(),
            access_token_ttl_secs: 0, // expires immediately
            refresh_token_ttl_secs: 604800,
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin123".to_string(),
        });
        let user_id = Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(auth_service))
                .route("/protected", web::get().to(protected_endpoint)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/protected")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_reject_invalid_token() {
        let auth_service = test_auth_service();

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(auth_service))
                .route("/protected", web::get().to(protected_endpoint)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/protected")
            .insert_header(("Authorization", "Bearer invalid.jwt.token"))
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_require_admin_as_admin() {
        let user = AuthenticatedUser {
            user_id: Uuid::new_v4(),
            role: "admin".to_string(),
        };
        assert!(user.require_admin().is_ok());
    }

    #[actix_rt::test]
    async fn test_require_admin_as_user() {
        let user = AuthenticatedUser {
            user_id: Uuid::new_v4(),
            role: "user".to_string(),
        };
        let err = user.require_admin().unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)));
    }

    #[actix_rt::test]
    async fn test_require_owner_or_admin_as_admin() {
        let user = AuthenticatedUser {
            user_id: Uuid::new_v4(),
            role: "admin".to_string(),
        };
        // Admin can access any resource, regardless of owner
        let other_id = Uuid::new_v4();
        assert!(user.require_owner_or_admin(Some(other_id)).is_ok());
        assert!(user.require_owner_or_admin(None).is_ok());
    }

    #[actix_rt::test]
    async fn test_require_owner_or_admin_as_owner() {
        let user_id = Uuid::new_v4();
        let user = AuthenticatedUser {
            user_id,
            role: "user".to_string(),
        };
        assert!(user.require_owner_or_admin(Some(user_id)).is_ok());
    }

    #[actix_rt::test]
    async fn test_require_owner_or_admin_as_non_owner() {
        let user = AuthenticatedUser {
            user_id: Uuid::new_v4(),
            role: "user".to_string(),
        };
        let other_id = Uuid::new_v4();
        let err = user.require_owner_or_admin(Some(other_id)).unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)));
    }

    #[actix_rt::test]
    async fn test_require_owner_or_admin_with_no_owner() {
        let user = AuthenticatedUser {
            user_id: Uuid::new_v4(),
            role: "user".to_string(),
        };
        let err = user.require_owner_or_admin(None).unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)));
    }
}
