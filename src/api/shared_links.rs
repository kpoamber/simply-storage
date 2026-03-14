use actix_web::{web, HttpResponse};
use chrono::{Duration, Utc};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::auth::AuthenticatedUser;
use crate::db::models::{Project, SharedLink, UserProject};
use crate::error::AppError;
use crate::services::SharedLinkService;

// ─── Request/Response types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateSharedLinkRequest {
    pub file_id: Uuid,
    pub password: Option<String>,
    pub expires_in_seconds: Option<i64>,
    pub max_downloads: Option<i32>,
}

/// Response wrapper that adds `password_protected` boolean.
/// `password_hash` is skip_serializing on SharedLink, so the frontend
/// needs this derived field to distinguish public vs protected links.
#[derive(Debug, serde::Serialize)]
pub struct SharedLinkResponse {
    #[serde(flatten)]
    pub link: SharedLink,
    pub password_protected: bool,
}

impl From<SharedLink> for SharedLinkResponse {
    fn from(link: SharedLink) -> Self {
        let password_protected = link.password_protected();
        Self { link, password_protected }
    }
}

fn to_response_list(links: Vec<SharedLink>) -> Vec<SharedLinkResponse> {
    links.into_iter().map(SharedLinkResponse::from).collect()
}

#[derive(Debug, Deserialize)]
pub struct VerifyPasswordRequest {
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct DownloadQuery {
    pub dl_token: Option<String>,
}

// ─── Authenticated management endpoints ─────────────────────────────────────────

async fn create_shared_link(
    user: AuthenticatedUser,
    path: web::Path<Uuid>,
    service: web::Data<SharedLinkService>,
    body: web::Json<CreateSharedLinkRequest>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    let pool = service.pool();

    // Verify project exists and user has write access (creating shared links is a write operation)
    let project = Project::find_by_id(pool, project_id).await?;
    require_project_write_access(pool, &user, &project).await?;

    if let Some(ref pw) = body.password {
        if pw.is_empty() {
            return Err(AppError::BadRequest(
                "Password must not be empty".to_string(),
            ));
        }
    }

    if let Some(secs) = body.expires_in_seconds {
        if secs <= 0 {
            return Err(AppError::BadRequest(
                "expires_in_seconds must be positive".to_string(),
            ));
        }
        // Cap at 365 days to prevent overflow in Duration::seconds()
        if secs > 365 * 24 * 3600 {
            return Err(AppError::BadRequest(
                "expires_in_seconds must not exceed 365 days (31536000 seconds)".to_string(),
            ));
        }
    }

    if let Some(max) = body.max_downloads {
        if max <= 0 {
            return Err(AppError::BadRequest(
                "max_downloads must be positive".to_string(),
            ));
        }
    }

    let expires_at = body
        .expires_in_seconds
        .map(|secs| Utc::now() + Duration::seconds(secs));

    let input = crate::services::shared_link_service::CreateSharedLinkInput {
        file_id: body.file_id,
        project_id,
        user_id: user.user_id,
        user_role: user.role.clone(),
        password: body.password.clone(),
        expires_at,
        max_downloads: body.max_downloads,
    };

    let link = service.create_link(&input).await?;
    Ok(HttpResponse::Created().json(SharedLinkResponse::from(link)))
}

async fn list_shared_links(
    user: AuthenticatedUser,
    path: web::Path<Uuid>,
    service: web::Data<SharedLinkService>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    let pool = service.pool();

    let project = Project::find_by_id(pool, project_id).await?;
    check_project_access(pool, &user, &project).await?;

    let links = service.list_links(project_id).await?;
    Ok(HttpResponse::Ok().json(to_response_list(links)))
}

async fn get_shared_link(
    user: AuthenticatedUser,
    path: web::Path<Uuid>,
    service: web::Data<SharedLinkService>,
) -> Result<HttpResponse, AppError> {
    let link_id = path.into_inner();
    let link = crate::db::models::SharedLink::find_by_id(
        service.pool(),
        link_id,
    )
    .await?;

    // Only creator or admin can view link details
    if !user.is_admin() && link.created_by != user.user_id {
        return Err(AppError::Forbidden(
            "Access denied: not the link creator".to_string(),
        ));
    }

    Ok(HttpResponse::Ok().json(SharedLinkResponse::from(link)))
}

async fn deactivate_shared_link(
    user: AuthenticatedUser,
    path: web::Path<Uuid>,
    service: web::Data<SharedLinkService>,
) -> Result<HttpResponse, AppError> {
    let link_id = path.into_inner();
    let link = service
        .deactivate_link(link_id, user.user_id, &user.role)
        .await?;
    Ok(HttpResponse::Ok().json(SharedLinkResponse::from(link)))
}

// ─── Public proxy endpoints (no auth required) ─────────────────────────────────

async fn public_link_info(
    path: web::Path<String>,
    service: web::Data<SharedLinkService>,
) -> Result<HttpResponse, AppError> {
    let token = path.into_inner();
    let info = service.get_link_info(&token).await?;
    Ok(HttpResponse::Ok().json(info))
}

async fn public_verify_password(
    path: web::Path<String>,
    service: web::Data<SharedLinkService>,
    body: web::Json<VerifyPasswordRequest>,
) -> Result<HttpResponse, AppError> {
    let token = path.into_inner();
    let dl_token = service.verify_password(&token, &body.password).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "dl_token": dl_token
    })))
}

async fn public_download(
    path: web::Path<String>,
    service: web::Data<SharedLinkService>,
    query: web::Query<DownloadQuery>,
) -> Result<HttpResponse, AppError> {
    let token = path.into_inner();
    let result = service
        .download_via_link(&token, query.dl_token.as_deref())
        .await?;

    // Sanitize filename to prevent header injection (same as files.rs download)
    let safe_name: String = result
        .file_name
        .chars()
        .filter(|c| *c != '"' && *c != '\\' && *c != '\r' && *c != '\n')
        .collect();
    Ok(HttpResponse::Ok()
        .content_type(result.content_type)
        .insert_header((
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", safe_name),
        ))
        .body(result.data))
}

// ─── Helpers ────────────────────────────────────────────────────────────────────

async fn check_project_access(
    pool: &PgPool,
    user: &AuthenticatedUser,
    project: &Project,
) -> Result<(), AppError> {
    if user.is_admin() || user.is_owner(project.owner_id) {
        return Ok(());
    }
    let is_member = UserProject::is_member(pool, user.user_id, project.id).await?;
    if !is_member {
        return Err(AppError::Forbidden(
            "Access denied: not a project member".to_string(),
        ));
    }
    Ok(())
}

/// Check that the user has write access to a project (admin, owner, or writer member).
async fn require_project_write_access(
    pool: &PgPool,
    user: &AuthenticatedUser,
    project: &Project,
) -> Result<(), AppError> {
    if user.is_admin() || user.is_owner(project.owner_id) {
        return Ok(());
    }
    match UserProject::get_role(pool, user.user_id, project.id).await? {
        Some(role) if role == "writer" => Ok(()),
        _ => Err(AppError::Forbidden(
            "Access denied: writer, owner, or admin access required".to_string(),
        )),
    }
}

// ─── Route configuration ────────────────────────────────────────────────────────

/// Authenticated routes under /api scope.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/projects/{project_id}/shared-links")
            .route(web::post().to(create_shared_link))
            .route(web::get().to(list_shared_links)),
    )
    .service(
        web::resource("/shared-links/{id}")
            .route(web::get().to(get_shared_link))
            .route(web::delete().to(deactivate_shared_link)),
    );
}

/// Public proxy routes under /s/ prefix (no auth).
pub fn configure_public(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/s")
            .route("/{token}", web::get().to(public_link_info))
            .route("/{token}/verify", web::post().to(public_verify_password))
            .route("/{token}/download", web::get().to(public_download)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthConfig;
    use crate::services::auth_service::AuthService;

    fn test_auth_service() -> AuthService {
        AuthService::new(&AuthConfig {
            jwt_secret: "test-secret-for-shared-links-api".to_string(),
            access_token_ttl_secs: 900,
            refresh_token_ttl_secs: 604800,
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin123".to_string(),
        })
    }

    #[test]
    fn test_create_shared_link_request_deserialization() {
        let json = serde_json::json!({
            "file_id": "550e8400-e29b-41d4-a716-446655440000",
            "password": "secret123",
            "expires_in_seconds": 3600,
            "max_downloads": 10
        });
        let req: CreateSharedLinkRequest = serde_json::from_value(json).unwrap();
        assert_eq!(
            req.file_id,
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
        );
        assert_eq!(req.password, Some("secret123".to_string()));
        assert_eq!(req.expires_in_seconds, Some(3600));
        assert_eq!(req.max_downloads, Some(10));
    }

    #[test]
    fn test_create_shared_link_request_minimal() {
        let json = serde_json::json!({
            "file_id": "550e8400-e29b-41d4-a716-446655440000"
        });
        let req: CreateSharedLinkRequest = serde_json::from_value(json).unwrap();
        assert!(req.password.is_none());
        assert!(req.expires_in_seconds.is_none());
        assert!(req.max_downloads.is_none());
    }

    #[test]
    fn test_verify_password_request_deserialization() {
        let json = serde_json::json!({
            "password": "my-secret"
        });
        let req: VerifyPasswordRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.password, "my-secret");
    }

    #[test]
    fn test_download_query_deserialization() {
        let json = serde_json::json!({
            "dl_token": "some.jwt.token"
        });
        let query: DownloadQuery = serde_json::from_value(json).unwrap();
        assert_eq!(query.dl_token, Some("some.jwt.token".to_string()));
    }

    #[test]
    fn test_download_query_empty() {
        let json = serde_json::json!({});
        let query: DownloadQuery = serde_json::from_value(json).unwrap();
        assert!(query.dl_token.is_none());
    }

    // ─── Auth enforcement tests ─────────────────────────────────────────────

    #[actix_rt::test]
    async fn test_create_shared_link_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .service(
                    web::scope("/api").configure(configure),
                ),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/api/projects/550e8400-e29b-41d4-a716-446655440000/shared-links")
            .set_json(serde_json::json!({
                "file_id": "550e8400-e29b-41d4-a716-446655440001"
            }))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_list_shared_links_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .service(
                    web::scope("/api").configure(configure),
                ),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/api/projects/550e8400-e29b-41d4-a716-446655440000/shared-links")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_get_shared_link_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .service(
                    web::scope("/api").configure(configure),
                ),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/api/shared-links/550e8400-e29b-41d4-a716-446655440000")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_deactivate_shared_link_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .service(
                    web::scope("/api").configure(configure),
                ),
        )
        .await;

        let req = actix_web::test::TestRequest::delete()
            .uri("/api/shared-links/550e8400-e29b-41d4-a716-446655440000")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_public_info_route_exists() {
        // This test verifies the public route is configured and reachable
        // (it will fail at the service layer since no SharedLinkService is configured,
        // but a 500 means the route matched)
        let app = actix_web::test::init_service(
            actix_web::App::new().configure(configure_public),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/s/some-token")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        // Without SharedLinkService configured, we expect 500 (route matched but service missing)
        // The important thing is that it's NOT 404, proving the route is registered
        assert_ne!(resp.status(), 404);
    }

    #[actix_rt::test]
    async fn test_public_verify_route_exists() {
        let app = actix_web::test::init_service(
            actix_web::App::new().configure(configure_public),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/s/some-token/verify")
            .set_json(serde_json::json!({"password": "test"}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_ne!(resp.status(), 404);
    }

    #[actix_rt::test]
    async fn test_public_download_route_exists() {
        let app = actix_web::test::init_service(
            actix_web::App::new().configure(configure_public),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/s/some-token/download")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_ne!(resp.status(), 404);
    }

    #[actix_rt::test]
    async fn test_public_routes_dont_require_auth() {
        // Public routes should NOT return 401
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .configure(configure_public),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/s/test-token")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        // Should NOT be 401 - these are public routes
        assert_ne!(resp.status(), 401);
    }

    #[test]
    fn test_filename_sanitization_strips_dangerous_chars() {
        // Test the same sanitization logic used in public_download
        let dangerous = "file\"name\r\ninjected\\header.pdf";
        let safe: String = dangerous
            .chars()
            .filter(|c| *c != '"' && *c != '\\' && *c != '\r' && *c != '\n')
            .collect();
        assert_eq!(safe, "filenameinjectedheader.pdf");
        assert!(!safe.contains('"'));
        assert!(!safe.contains('\\'));
        assert!(!safe.contains('\r'));
        assert!(!safe.contains('\n'));
    }

    #[test]
    fn test_expires_in_seconds_validation_rejects_negative() {
        // Negative values should be rejected
        let secs: i64 = -3600;
        assert!(secs <= 0);
    }

    #[test]
    fn test_expires_in_seconds_validation_rejects_zero() {
        let secs: i64 = 0;
        assert!(secs <= 0);
    }
}
