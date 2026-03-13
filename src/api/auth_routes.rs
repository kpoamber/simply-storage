use actix_web::{web, HttpResponse};
use chrono::{Duration, Utc};
use serde::Deserialize;
use sqlx::PgPool;

use crate::api::auth::AuthenticatedUser;
use crate::db::models::{is_unique_violation, CreateRefreshToken, CreateUser, Project, RefreshToken, User, UserProject, UserStorage};
use crate::error::AppError;
use crate::services::auth_service::AuthService;

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    #[serde(default = "default_user_role")]
    pub role: String,
}

fn default_user_role() -> String {
    "user".to_string()
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub role: Option<String>,
    pub password: Option<String>,
}

/// Admin-only: create a new user
async fn create_user(
    pool: web::Data<PgPool>,
    auth_service: web::Data<AuthService>,
    auth_user: AuthenticatedUser,
    body: web::Json<CreateUserRequest>,
) -> Result<HttpResponse, AppError> {
    auth_user.require_admin()?;

    if body.username.is_empty() || body.password.is_empty() {
        return Err(AppError::BadRequest(
            "Username and password are required".to_string(),
        ));
    }

    let username = body.username.trim();
    if username.len() < 3 || username.len() > 64 {
        return Err(AppError::BadRequest(
            "Username must be between 3 and 64 characters".to_string(),
        ));
    }
    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') {
        return Err(AppError::BadRequest(
            "Username may only contain letters, digits, underscores, hyphens, and dots".to_string(),
        ));
    }

    if body.password.len() < 6 {
        return Err(AppError::BadRequest(
            "Password must be at least 6 characters".to_string(),
        ));
    }

    if body.password.len() > 1024 {
        return Err(AppError::BadRequest(
            "Password must not exceed 1024 characters".to_string(),
        ));
    }

    if body.role != "admin" && body.role != "user" {
        return Err(AppError::BadRequest(
            "Role must be 'admin' or 'user'".to_string(),
        ));
    }

    let password_hash = auth_service
        .hash_password(&body.password)
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?;

    let user = match User::create(
        pool.get_ref(),
        &CreateUser {
            username: username.to_string(),
            password_hash,
            role: body.role.clone(),
        },
    )
    .await
    {
        Ok(user) => user,
        Err(AppError::Database(ref e)) if is_unique_violation(e) => {
            return Err(AppError::Conflict(
                "Username already exists".to_string(),
            ));
        }
        Err(e) => return Err(e),
    };

    Ok(HttpResponse::Created().json(user))
}

/// Admin-only: list all users
async fn list_users(
    pool: web::Data<PgPool>,
    auth_user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    auth_user.require_admin()?;
    let users = User::list(pool.get_ref()).await?;
    Ok(HttpResponse::Ok().json(users))
}

/// Admin-only: delete a user
async fn delete_user(
    pool: web::Data<PgPool>,
    auth_user: AuthenticatedUser,
    path: web::Path<uuid::Uuid>,
) -> Result<HttpResponse, AppError> {
    auth_user.require_admin()?;
    let user_id = path.into_inner();

    if user_id == auth_user.user_id {
        return Err(AppError::BadRequest(
            "Cannot delete your own account".to_string(),
        ));
    }

    // Check if user owns any projects - must be reassigned first
    let owned_projects = Project::list_for_owner(pool.get_ref(), user_id).await?;
    if !owned_projects.is_empty() {
        let names: Vec<String> = owned_projects.iter().map(|p| p.name.clone()).collect();
        return Err(AppError::Conflict(format!(
            "Cannot delete user: they own {} project(s) ({}). Reassign ownership first.",
            owned_projects.len(),
            names.join(", ")
        )));
    }

    // Delete user's refresh tokens first (junction tables cascade automatically)
    RefreshToken::delete_by_user_id(pool.get_ref(), user_id).await?;
    User::delete(pool.get_ref(), user_id).await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({"message": "User deleted"})))
}

async fn login(
    pool: web::Data<PgPool>,
    auth_service: web::Data<AuthService>,
    body: web::Json<LoginRequest>,
) -> Result<HttpResponse, AppError> {
    let user = User::find_by_username(pool.get_ref(), &body.username)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Invalid username or password".to_string()))?;

    if !auth_service.verify_password(&body.password, &user.password_hash) {
        return Err(AppError::Unauthorized(
            "Invalid username or password".to_string(),
        ));
    }

    let access_token = auth_service
        .generate_access_token(user.id, &user.role)
        .map_err(|e| AppError::Internal(format!("Failed to generate access token: {}", e)))?;

    let refresh_token = auth_service.generate_refresh_token();
    let refresh_token_hash = AuthService::hash_refresh_token(&refresh_token);

    let expires_at =
        Utc::now() + Duration::seconds(auth_service.refresh_token_ttl_secs as i64);

    RefreshToken::create(
        pool.get_ref(),
        &CreateRefreshToken {
            user_id: user.id,
            token_hash: refresh_token_hash,
            expires_at,
        },
    )
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
    })))
}

async fn refresh(
    pool: web::Data<PgPool>,
    auth_service: web::Data<AuthService>,
    body: web::Json<RefreshRequest>,
) -> Result<HttpResponse, AppError> {
    let token_hash = AuthService::hash_refresh_token(&body.refresh_token);

    // Atomically consume the refresh token (delete + return in one query).
    // This prevents race conditions where two concurrent requests both use
    // the same refresh token before either deletes it.
    let stored_token = RefreshToken::consume_by_hash(pool.get_ref(), &token_hash)
        .await?
        .ok_or_else(|| {
            AppError::Unauthorized("Invalid or expired refresh token".to_string())
        })?;

    // Get user - if account was deleted, return 401 (not 404)
    let user = match User::find_by_id(pool.get_ref(), stored_token.user_id).await {
        Ok(u) => u,
        Err(AppError::NotFound(_)) => {
            return Err(AppError::Unauthorized("Account no longer exists".to_string()));
        }
        Err(e) => return Err(e),
    };

    // Generate new tokens
    let access_token = auth_service
        .generate_access_token(user.id, &user.role)
        .map_err(|e| AppError::Internal(format!("Failed to generate access token: {}", e)))?;

    let new_refresh_token = auth_service.generate_refresh_token();
    let new_refresh_token_hash = AuthService::hash_refresh_token(&new_refresh_token);

    let expires_at =
        Utc::now() + Duration::seconds(auth_service.refresh_token_ttl_secs as i64);

    RefreshToken::create(
        pool.get_ref(),
        &CreateRefreshToken {
            user_id: user.id,
            token_hash: new_refresh_token_hash,
            expires_at,
        },
    )
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "access_token": access_token,
        "refresh_token": new_refresh_token,
    })))
}

async fn me(
    pool: web::Data<PgPool>,
    auth_user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let user = User::find_by_id(pool.get_ref(), auth_user.user_id).await?;
    Ok(HttpResponse::Ok().json(user))
}

async fn logout(
    pool: web::Data<PgPool>,
    body: web::Json<LogoutRequest>,
) -> Result<HttpResponse, AppError> {
    let token_hash = AuthService::hash_refresh_token(&body.refresh_token);
    RefreshToken::delete_by_hash(pool.get_ref(), &token_hash).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({"message": "Logged out successfully"})))
}

/// Admin-only: get user detail with assigned projects and storages
async fn get_user(
    pool: web::Data<PgPool>,
    auth_user: AuthenticatedUser,
    path: web::Path<uuid::Uuid>,
) -> Result<HttpResponse, AppError> {
    auth_user.require_admin()?;
    let user_id = path.into_inner();

    let user = User::find_by_id(pool.get_ref(), user_id).await?;
    let projects = UserProject::list_for_user(pool.get_ref(), user_id).await?;
    let storages: Vec<_> = UserStorage::list_for_user(pool.get_ref(), user_id)
        .await?
        .into_iter()
        .map(|s| s.redacted())
        .collect();

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "user": user,
        "projects": projects,
        "storages": storages,
    })))
}

/// Admin-only: update user role and/or password
async fn update_user(
    pool: web::Data<PgPool>,
    auth_service: web::Data<AuthService>,
    auth_user: AuthenticatedUser,
    path: web::Path<uuid::Uuid>,
    body: web::Json<UpdateUserRequest>,
) -> Result<HttpResponse, AppError> {
    auth_user.require_admin()?;
    let user_id = path.into_inner();

    if body.role.is_none() && body.password.is_none() {
        return Err(AppError::BadRequest(
            "At least one of 'role' or 'password' must be provided".to_string(),
        ));
    }

    // Ensure the target user exists
    let user = User::find_by_id(pool.get_ref(), user_id).await?;

    let mut updated_user = user;

    // Validate all inputs before making any changes
    if let Some(ref role) = body.role {
        if role != "admin" && role != "user" {
            return Err(AppError::BadRequest(
                "Role must be 'admin' or 'user'".to_string(),
            ));
        }

        // Prevent admin from demoting themselves
        if user_id == auth_user.user_id && role != "admin" {
            return Err(AppError::BadRequest(
                "Cannot demote your own admin account".to_string(),
            ));
        }
    }

    let password_hash = if let Some(ref password) = body.password {
        if password.len() < 6 {
            return Err(AppError::BadRequest(
                "Password must be at least 6 characters".to_string(),
            ));
        }
        if password.len() > 1024 {
            return Err(AppError::BadRequest(
                "Password must not exceed 1024 characters".to_string(),
            ));
        }
        Some(
            auth_service
                .hash_password(password)
                .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?,
        )
    } else {
        None
    };

    // Apply updates
    if let Some(ref role) = body.role {
        updated_user = User::update_role(pool.get_ref(), user_id, role).await?;
    }

    if let Some(ref hash) = password_hash {
        updated_user = User::update_password_hash(pool.get_ref(), user_id, hash).await?;
    }

    // Invalidate refresh tokens once if either role or password changed
    if body.role.is_some() || body.password.is_some() {
        RefreshToken::delete_by_user_id(pool.get_ref(), user_id).await?;
    }

    Ok(HttpResponse::Ok().json(updated_user))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/auth")
            .route("/login", web::post().to(login))
            .route("/refresh", web::post().to(refresh))
            .route("/me", web::get().to(me))
            .route("/logout", web::post().to(logout))
            .route("/users", web::post().to(create_user))
            .route("/users", web::get().to(list_users))
            .route("/users/{user_id}", web::get().to(get_user))
            .route("/users/{user_id}", web::put().to(update_user))
            .route("/users/{user_id}", web::delete().to(delete_user)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::App;
    use crate::config::AuthConfig;

    fn test_auth_service() -> AuthService {
        AuthService::new(&AuthConfig {
            jwt_secret: "test-secret-for-auth-routes".to_string(),
            access_token_ttl_secs: 900,
            refresh_token_ttl_secs: 604800,
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin123".to_string(),
        })
    }

    #[test]
    fn test_create_user_request_deserialization() {
        let json = serde_json::json!({
            "username": "alice",
            "password": "secret123"
        });
        let req: CreateUserRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.username, "alice");
        assert_eq!(req.password, "secret123");
        assert_eq!(req.role, "user"); // default role
    }

    #[test]
    fn test_create_user_request_with_role() {
        let json = serde_json::json!({
            "username": "alice",
            "password": "secret123",
            "role": "admin"
        });
        let req: CreateUserRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.role, "admin");
    }

    #[test]
    fn test_login_request_deserialization() {
        let json = serde_json::json!({
            "username": "bob",
            "password": "pass123"
        });
        let req: LoginRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.username, "bob");
        assert_eq!(req.password, "pass123");
    }

    #[test]
    fn test_refresh_request_deserialization() {
        let json = serde_json::json!({
            "refresh_token": "abc123def456"
        });
        let req: RefreshRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.refresh_token, "abc123def456");
    }

    #[test]
    fn test_logout_request_deserialization() {
        let json = serde_json::json!({
            "refresh_token": "token_to_revoke"
        });
        let req: LogoutRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.refresh_token, "token_to_revoke");
    }

    #[test]
    fn test_update_user_request_deserialization() {
        let json = serde_json::json!({
            "role": "admin",
            "password": "newpass123"
        });
        let req: UpdateUserRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.role, Some("admin".to_string()));
        assert_eq!(req.password, Some("newpass123".to_string()));
    }

    #[test]
    fn test_update_user_request_partial() {
        let json = serde_json::json!({ "role": "user" });
        let req: UpdateUserRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.role, Some("user".to_string()));
        assert!(req.password.is_none());

        let json = serde_json::json!({ "password": "newpass" });
        let req: UpdateUserRequest = serde_json::from_value(json).unwrap();
        assert!(req.role.is_none());
        assert_eq!(req.password, Some("newpass".to_string()));
    }

    #[test]
    fn test_update_user_request_empty() {
        let json = serde_json::json!({});
        let req: UpdateUserRequest = serde_json::from_value(json).unwrap();
        assert!(req.role.is_none());
        assert!(req.password.is_none());
    }

    #[test]
    fn test_create_user_request_missing_fields() {
        let json = serde_json::json!({
            "username": "alice"
        });
        let result: Result<CreateUserRequest, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_login_request_missing_fields() {
        let json = serde_json::json!({
            "password": "pass123"
        });
        let result: Result<LoginRequest, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[actix_rt::test]
    async fn test_me_endpoint_requires_auth() {
        let auth_service = test_auth_service();

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/api/auth/me")
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_me_endpoint_rejects_invalid_token() {
        let auth_service = test_auth_service();

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/api/auth/me")
            .insert_header(("Authorization", "Bearer invalid.token.here"))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_create_user_requires_auth() {
        let auth_service = test_auth_service();

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/users")
            .set_json(serde_json::json!({
                "username": "newuser",
                "password": "password123"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_routes_are_registered() {
        let auth_service = test_auth_service();

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        // POST /api/auth/login should not be 404
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/login")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_ne!(resp.status(), 404);

        // POST /api/auth/refresh should not be 404
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/refresh")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_ne!(resp.status(), 404);

        // POST /api/auth/logout should not be 404
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/logout")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_ne!(resp.status(), 404);

        // POST /api/auth/users should not be 404
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/users")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_ne!(resp.status(), 404);

        // GET /api/auth/users should not be 404
        let req = actix_web::test::TestRequest::get()
            .uri("/api/auth/users")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_ne!(resp.status(), 404);

        // GET /api/auth/users/{id} should not be 404
        let fake_id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::get()
            .uri(&format!("/api/auth/users/{}", fake_id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_ne!(resp.status(), 404);

        // PUT /api/auth/users/{id} should not be 404
        let req = actix_web::test::TestRequest::put()
            .uri(&format!("/api/auth/users/{}", fake_id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_ne!(resp.status(), 404);
    }

    // ─── Integration tests that require a running PostgreSQL ───────────────────
    // Run with: DATABASE_URL=postgres://... cargo test -- --ignored

    /// Helper: create a user directly in the DB and return (user, access_token)
    async fn seed_test_user(pool: &PgPool, auth_service: &AuthService, username: &str, role: &str) -> (User, String) {
        let password_hash = auth_service.hash_password("password123").unwrap();
        let user = User::create(pool, &CreateUser {
            username: username.to_string(),
            password_hash,
            role: role.to_string(),
        }).await.unwrap();
        let access_token = auth_service.generate_access_token(user.id, &user.role).unwrap();
        (user, access_token)
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_create_user_requires_admin() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let username = format!("test_regular_{}", uuid::Uuid::new_v4());
        let (_, user_token) = seed_test_user(&pool, &auth_service, &username, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        // Regular user cannot create users
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/users")
            .insert_header(("Authorization", format!("Bearer {}", user_token)))
            .set_json(serde_json::json!({
                "username": "newuser",
                "password": "password123"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_admin_creates_user() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let admin_name = format!("test_admin_{}", uuid::Uuid::new_v4());
        let (_, admin_token) = seed_test_user(&pool, &auth_service, &admin_name, "admin").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let new_username = format!("test_new_{}", uuid::Uuid::new_v4());
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/users")
            .insert_header(("Authorization", format!("Bearer {}", admin_token)))
            .set_json(serde_json::json!({
                "username": new_username,
                "password": "password123"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 201);

        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        assert_eq!(body["username"], new_username);
        assert_eq!(body["role"], "user");
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_login_success() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let username = format!("test_login_{}", uuid::Uuid::new_v4());
        seed_test_user(&pool, &auth_service, &username, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(serde_json::json!({
                "username": username,
                "password": "password123"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 200);

        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        assert!(!body["access_token"].as_str().unwrap().is_empty());
        assert!(!body["refresh_token"].as_str().unwrap().is_empty());
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_login_wrong_password() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let username = format!("test_wrong_{}", uuid::Uuid::new_v4());
        seed_test_user(&pool, &auth_service, &username, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(serde_json::json!({
                "username": username,
                "password": "wrongpassword"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_login_nonexistent_user() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(serde_json::json!({
                "username": "nonexistent_user_xyz",
                "password": "password123"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_refresh_with_invalid_token() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/refresh")
            .set_json(serde_json::json!({
                "refresh_token": "invalid_refresh_token"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_me_endpoint_with_valid_token() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let username = format!("test_me_{}", uuid::Uuid::new_v4());
        let (_, access_token) = seed_test_user(&pool, &auth_service, &username, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/api/auth/me")
            .insert_header(("Authorization", format!("Bearer {}", access_token)))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 200);

        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        assert_eq!(body["username"], username);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_get_user_detail() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let admin_name = format!("test_admin_{}", uuid::Uuid::new_v4());
        let (_, admin_token) = seed_test_user(&pool, &auth_service, &admin_name, "admin").await;
        let target_name = format!("test_target_{}", uuid::Uuid::new_v4());
        let (target_user, _) = seed_test_user(&pool, &auth_service, &target_name, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri(&format!("/api/auth/users/{}", target_user.id))
            .insert_header(("Authorization", format!("Bearer {}", admin_token)))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 200);

        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        assert_eq!(body["user"]["username"], target_name);
        assert!(body["projects"].is_array());
        assert!(body["storages"].is_array());
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_get_user_detail_requires_admin() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let user_name = format!("test_user_{}", uuid::Uuid::new_v4());
        let (user, user_token) = seed_test_user(&pool, &auth_service, &user_name, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri(&format!("/api/auth/users/{}", user.id))
            .insert_header(("Authorization", format!("Bearer {}", user_token)))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_get_user_detail_not_found() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let admin_name = format!("test_admin_{}", uuid::Uuid::new_v4());
        let (_, admin_token) = seed_test_user(&pool, &auth_service, &admin_name, "admin").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let fake_id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::get()
            .uri(&format!("/api/auth/users/{}", fake_id))
            .insert_header(("Authorization", format!("Bearer {}", admin_token)))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 404);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_update_user_role() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let admin_name = format!("test_admin_{}", uuid::Uuid::new_v4());
        let (_, admin_token) = seed_test_user(&pool, &auth_service, &admin_name, "admin").await;
        let target_name = format!("test_target_{}", uuid::Uuid::new_v4());
        let (target_user, _) = seed_test_user(&pool, &auth_service, &target_name, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        // Promote to admin
        let req = actix_web::test::TestRequest::put()
            .uri(&format!("/api/auth/users/{}", target_user.id))
            .insert_header(("Authorization", format!("Bearer {}", admin_token)))
            .set_json(serde_json::json!({ "role": "admin" }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 200);

        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        assert_eq!(body["role"], "admin");
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_update_user_password() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let admin_name = format!("test_admin_{}", uuid::Uuid::new_v4());
        let (_, admin_token) = seed_test_user(&pool, &auth_service, &admin_name, "admin").await;
        let target_name = format!("test_target_{}", uuid::Uuid::new_v4());
        let (target_user, _) = seed_test_user(&pool, &auth_service, &target_name, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service.clone()))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        // Reset password
        let req = actix_web::test::TestRequest::put()
            .uri(&format!("/api/auth/users/{}", target_user.id))
            .insert_header(("Authorization", format!("Bearer {}", admin_token)))
            .set_json(serde_json::json!({ "password": "newpassword123" }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 200);

        // Verify new password works via login
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(serde_json::json!({
                "username": target_name,
                "password": "newpassword123"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 200);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_update_user_invalid_role() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let admin_name = format!("test_admin_{}", uuid::Uuid::new_v4());
        let (_, admin_token) = seed_test_user(&pool, &auth_service, &admin_name, "admin").await;
        let target_name = format!("test_target_{}", uuid::Uuid::new_v4());
        let (target_user, _) = seed_test_user(&pool, &auth_service, &target_name, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::put()
            .uri(&format!("/api/auth/users/{}", target_user.id))
            .insert_header(("Authorization", format!("Bearer {}", admin_token)))
            .set_json(serde_json::json!({ "role": "superadmin" }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 400);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_update_user_short_password() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let admin_name = format!("test_admin_{}", uuid::Uuid::new_v4());
        let (_, admin_token) = seed_test_user(&pool, &auth_service, &admin_name, "admin").await;
        let target_name = format!("test_target_{}", uuid::Uuid::new_v4());
        let (target_user, _) = seed_test_user(&pool, &auth_service, &target_name, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::put()
            .uri(&format!("/api/auth/users/{}", target_user.id))
            .insert_header(("Authorization", format!("Bearer {}", admin_token)))
            .set_json(serde_json::json!({ "password": "short" }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 400);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_update_user_cannot_self_demote() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let admin_name = format!("test_admin_{}", uuid::Uuid::new_v4());
        let (admin_user, admin_token) = seed_test_user(&pool, &auth_service, &admin_name, "admin").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::put()
            .uri(&format!("/api/auth/users/{}", admin_user.id))
            .insert_header(("Authorization", format!("Bearer {}", admin_token)))
            .set_json(serde_json::json!({ "role": "user" }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 400);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_update_user_requires_admin() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let user_name = format!("test_user_{}", uuid::Uuid::new_v4());
        let (user, user_token) = seed_test_user(&pool, &auth_service, &user_name, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::put()
            .uri(&format!("/api/auth/users/{}", user.id))
            .insert_header(("Authorization", format!("Bearer {}", user_token)))
            .set_json(serde_json::json!({ "role": "admin" }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_update_user_empty_body() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let auth_service = test_auth_service();
        let admin_name = format!("test_admin_{}", uuid::Uuid::new_v4());
        let (_, admin_token) = seed_test_user(&pool, &auth_service, &admin_name, "admin").await;
        let target_name = format!("test_target_{}", uuid::Uuid::new_v4());
        let (target_user, _) = seed_test_user(&pool, &auth_service, &target_name, "user").await;

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::put()
            .uri(&format!("/api/auth/users/{}", target_user.id))
            .insert_header(("Authorization", format!("Bearer {}", admin_token)))
            .set_json(serde_json::json!({}))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 400);
    }
}
