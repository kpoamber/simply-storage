use actix_web::{web, HttpResponse};
use chrono::{Duration, Utc};
use serde::Deserialize;
use sqlx::PgPool;

use crate::api::auth::AuthenticatedUser;
use crate::db::models::{CreateRefreshToken, CreateUser, RefreshToken, User};
use crate::error::AppError;
use crate::services::auth_service::AuthService;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
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

async fn register(
    pool: web::Data<PgPool>,
    auth_service: web::Data<AuthService>,
    body: web::Json<RegisterRequest>,
) -> Result<HttpResponse, AppError> {
    if body.username.is_empty() || body.password.is_empty() {
        return Err(AppError::BadRequest(
            "Username and password are required".to_string(),
        ));
    }

    if body.password.len() < 6 {
        return Err(AppError::BadRequest(
            "Password must be at least 6 characters".to_string(),
        ));
    }

    // Check if username already exists
    if User::find_by_username(pool.get_ref(), &body.username).await?.is_some() {
        return Err(AppError::Conflict(
            "Username already exists".to_string(),
        ));
    }

    // First registered user gets admin role
    let user_count = User::count(pool.get_ref()).await?;
    let role = if user_count == 0 { "admin" } else { "user" };

    let password_hash = auth_service
        .hash_password(&body.password)
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?;

    let user = User::create(
        pool.get_ref(),
        &CreateUser {
            username: body.username.clone(),
            password_hash,
            role: role.to_string(),
        },
    )
    .await?;

    // Generate tokens
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

    Ok(HttpResponse::Created().json(serde_json::json!({
        "user": user,
        "access_token": access_token,
        "refresh_token": refresh_token,
    })))
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

    let stored_token = RefreshToken::find_by_hash(pool.get_ref(), &token_hash)
        .await?
        .ok_or_else(|| {
            AppError::Unauthorized("Invalid or expired refresh token".to_string())
        })?;

    // Delete old refresh token
    RefreshToken::delete_by_hash(pool.get_ref(), &token_hash).await?;

    // Get user
    let user = User::find_by_id(pool.get_ref(), stored_token.user_id).await?;

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

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/auth")
            .route("/register", web::post().to(register))
            .route("/login", web::post().to(login))
            .route("/refresh", web::post().to(refresh))
            .route("/me", web::get().to(me))
            .route("/logout", web::post().to(logout)),
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
        })
    }

    #[test]
    fn test_register_request_deserialization() {
        let json = serde_json::json!({
            "username": "alice",
            "password": "secret123"
        });
        let req: RegisterRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.username, "alice");
        assert_eq!(req.password, "secret123");
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
    fn test_register_request_missing_fields() {
        let json = serde_json::json!({
            "username": "alice"
        });
        let result: Result<RegisterRequest, _> = serde_json::from_value(json);
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
    async fn test_routes_are_registered() {
        let auth_service = test_auth_service();

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        // POST /api/auth/login should not be 404 (it will be 400 due to missing body but not 404)
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/login")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_ne!(resp.status(), 404);

        // POST /api/auth/register should not be 404
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/register")
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
    }

    // ─── Integration tests that require a running PostgreSQL ───────────────────
    // Run with: DATABASE_URL=postgres://... cargo test -- --ignored

    #[ignore]
    #[actix_rt::test]
    async fn test_register_first_user_becomes_admin() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Clean up test users
        sqlx::query("DELETE FROM users WHERE username LIKE 'test_%'")
            .execute(&pool)
            .await
            .unwrap();

        let auth_service = test_auth_service();

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(serde_json::json!({
                "username": "test_admin_first",
                "password": "password123"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 201);

        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        assert_eq!(body["user"]["role"], "admin");
        assert!(!body["access_token"].as_str().unwrap().is_empty());
        assert!(!body["refresh_token"].as_str().unwrap().is_empty());
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_register_second_user_becomes_user() {
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

        // Ensure at least one user exists
        let count = User::count(&pool).await.unwrap();
        if count == 0 {
            let req = actix_web::test::TestRequest::post()
                .uri("/api/auth/register")
                .set_json(serde_json::json!({
                    "username": "test_ensure_admin",
                    "password": "password123"
                }))
                .to_request();
            actix_web::test::call_service(&app, req).await;
        }

        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(serde_json::json!({
                "username": format!("test_user_{}", uuid::Uuid::new_v4()),
                "password": "password123"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 201);

        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        assert_eq!(body["user"]["role"], "user");
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_register_duplicate_username() {
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

        let username = format!("test_dup_{}", uuid::Uuid::new_v4());

        // First registration
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(serde_json::json!({
                "username": username,
                "password": "password123"
            }))
            .to_request();
        actix_web::test::call_service(&app, req).await;

        // Second with same username
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(serde_json::json!({
                "username": username,
                "password": "password456"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 409);
    }

    #[ignore]
    #[actix_rt::test]
    async fn test_login_success() {
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

        let username = format!("test_login_{}", uuid::Uuid::new_v4());

        // Register first
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(serde_json::json!({
                "username": username,
                "password": "password123"
            }))
            .to_request();
        actix_web::test::call_service(&app, req).await;

        // Login
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

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let username = format!("test_wrong_{}", uuid::Uuid::new_v4());

        // Register
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(serde_json::json!({
                "username": username,
                "password": "password123"
            }))
            .to_request();
        actix_web::test::call_service(&app, req).await;

        // Login with wrong password
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
    async fn test_refresh_token_flow() {
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

        let username = format!("test_refresh_{}", uuid::Uuid::new_v4());

        // Register to get tokens
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(serde_json::json!({
                "username": username,
                "password": "password123"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        let refresh_token = body["refresh_token"].as_str().unwrap().to_string();

        // Refresh
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/refresh")
            .set_json(serde_json::json!({
                "refresh_token": refresh_token
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 200);

        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        assert!(!body["access_token"].as_str().unwrap().is_empty());
        assert!(!body["refresh_token"].as_str().unwrap().is_empty());
        // New refresh token should be different from old one
        assert_ne!(body["refresh_token"].as_str().unwrap(), refresh_token);
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

        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(auth_service))
                .service(web::scope("/api").configure(configure)),
        )
        .await;

        let username = format!("test_me_{}", uuid::Uuid::new_v4());

        // Register to get access token
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(serde_json::json!({
                "username": username,
                "password": "password123"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        let access_token = body["access_token"].as_str().unwrap().to_string();

        // GET /me
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
    async fn test_logout_invalidates_refresh_token() {
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

        let username = format!("test_logout_{}", uuid::Uuid::new_v4());

        // Register to get tokens
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(serde_json::json!({
                "username": username,
                "password": "password123"
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
        let refresh_token = body["refresh_token"].as_str().unwrap().to_string();

        // Logout
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/logout")
            .set_json(serde_json::json!({
                "refresh_token": refresh_token
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 200);

        // Try to use the refresh token after logout
        let req = actix_web::test::TestRequest::post()
            .uri("/api/auth/refresh")
            .set_json(serde_json::json!({
                "refresh_token": refresh_token
            }))
            .to_request();

        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }
}
