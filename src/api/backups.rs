use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use super::auth::AdminUser;
use crate::db::models::{
    BackupConfig, BackupRecord, CreateBackupConfig, Storage, UpdateBackupConfig,
};
use crate::error::AppError;
use crate::services::BackupService;

// ─── Response types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BackupConfigWithStorage {
    #[serde(flatten)]
    pub config: BackupConfig,
    pub storage_name: Option<String>,
}

// ─── Request types ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TriggerBackupRequest {
    pub config_id: Option<Uuid>,
    pub storage_id: Option<Uuid>,
    pub storage_path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BackupHistoryFilter {
    pub config_id: Option<Uuid>,
}

// ─── Backup config handlers ─────────────────────────────────────────────────────

async fn list_backup_configs(
    _admin: AdminUser,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, AppError> {
    let configs = BackupConfig::list(pool.get_ref()).await?;

    let mut result = Vec::with_capacity(configs.len());
    for config in configs {
        let storage_name = Storage::find_by_id(pool.get_ref(), config.storage_id)
            .await
            .ok()
            .map(|s| s.name);
        result.push(BackupConfigWithStorage {
            config,
            storage_name,
        });
    }

    Ok(HttpResponse::Ok().json(result))
}

async fn create_backup_config(
    _admin: AdminUser,
    pool: web::Data<PgPool>,
    body: web::Json<CreateBackupConfig>,
) -> Result<HttpResponse, AppError> {
    // Validate cron expression
    BackupService::validate_cron(&body.schedule_cron)?;

    // Validate storage exists
    Storage::find_by_id(pool.get_ref(), body.storage_id).await?;

    let config = BackupConfig::create(pool.get_ref(), &body).await?;
    Ok(HttpResponse::Created().json(config))
}

async fn update_backup_config(
    _admin: AdminUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateBackupConfig>,
) -> Result<HttpResponse, AppError> {
    let config_id = path.into_inner();

    // Validate cron expression if provided
    if let Some(ref cron_expr) = body.schedule_cron {
        BackupService::validate_cron(cron_expr)?;
    }

    // Validate storage exists if provided
    if let Some(storage_id) = body.storage_id {
        Storage::find_by_id(pool.get_ref(), storage_id).await?;
    }

    let config = BackupConfig::update(pool.get_ref(), config_id, &body).await?;
    Ok(HttpResponse::Ok().json(config))
}

async fn delete_backup_config(
    _admin: AdminUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let config_id = path.into_inner();
    BackupConfig::delete(pool.get_ref(), config_id).await?;
    Ok(HttpResponse::NoContent().finish())
}

// ─── Backup history handlers ────────────────────────────────────────────────────

async fn list_backups(
    _admin: AdminUser,
    pool: web::Data<PgPool>,
    query: web::Query<BackupHistoryFilter>,
) -> Result<HttpResponse, AppError> {
    let records = BackupRecord::list(pool.get_ref(), query.config_id).await?;
    Ok(HttpResponse::Ok().json(records))
}

async fn trigger_backup(
    _admin: AdminUser,
    pool: web::Data<PgPool>,
    service: web::Data<BackupService>,
    body: web::Json<TriggerBackupRequest>,
) -> Result<HttpResponse, AppError> {
    let (config_id, storage_id, storage_path) = if let Some(cfg_id) = body.config_id {
        // Use config settings
        let config = BackupConfig::find_by_id(pool.get_ref(), cfg_id).await?;
        (Some(cfg_id), config.storage_id, config.storage_path)
    } else if let Some(sid) = body.storage_id {
        // Manual backup with explicit storage
        Storage::find_by_id(pool.get_ref(), sid).await?;
        let path = body.storage_path.clone().unwrap_or_default();
        (None, sid, path)
    } else {
        return Err(AppError::BadRequest(
            "Either config_id or storage_id must be provided".to_string(),
        ));
    };

    let record = service
        .create_backup(config_id, storage_id, &storage_path)
        .await?;
    Ok(HttpResponse::Created().json(record))
}

async fn delete_backup(
    _admin: AdminUser,
    path: web::Path<Uuid>,
    service: web::Data<BackupService>,
) -> Result<HttpResponse, AppError> {
    let backup_id = path.into_inner();
    service.delete_backup(backup_id).await?;
    Ok(HttpResponse::NoContent().finish())
}

// ─── Route configuration ────────────────────────────────────────────────────────

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/backup-configs")
            .route(web::get().to(list_backup_configs))
            .route(web::post().to(create_backup_config)),
    )
    .service(
        web::resource("/backup-configs/{id}")
            .route(web::put().to(update_backup_config))
            .route(web::delete().to(delete_backup_config)),
    )
    .service(
        web::resource("/backups")
            .route(web::get().to(list_backups)),
    )
    .service(
        web::resource("/backups/trigger")
            .route(web::post().to(trigger_backup)),
    )
    .service(
        web::resource("/backups/{id}")
            .route(web::delete().to(delete_backup)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trigger_backup_request_with_config_id() {
        let json = serde_json::json!({
            "config_id": "550e8400-e29b-41d4-a716-446655440000"
        });
        let req: TriggerBackupRequest = serde_json::from_value(json).unwrap();
        assert!(req.config_id.is_some());
        assert!(req.storage_id.is_none());
        assert!(req.storage_path.is_none());
    }

    #[test]
    fn test_trigger_backup_request_with_storage() {
        let json = serde_json::json!({
            "storage_id": "550e8400-e29b-41d4-a716-446655440000",
            "storage_path": "backups/manual"
        });
        let req: TriggerBackupRequest = serde_json::from_value(json).unwrap();
        assert!(req.config_id.is_none());
        assert!(req.storage_id.is_some());
        assert_eq!(req.storage_path.as_deref(), Some("backups/manual"));
    }

    #[test]
    fn test_trigger_backup_request_empty() {
        let json = serde_json::json!({});
        let req: TriggerBackupRequest = serde_json::from_value(json).unwrap();
        assert!(req.config_id.is_none());
        assert!(req.storage_id.is_none());
    }

    #[test]
    fn test_backup_history_filter_deserialization() {
        let json = serde_json::json!({
            "config_id": "550e8400-e29b-41d4-a716-446655440000"
        });
        let filter: BackupHistoryFilter = serde_json::from_value(json).unwrap();
        assert!(filter.config_id.is_some());
    }

    #[test]
    fn test_backup_history_filter_empty() {
        let json = serde_json::json!({});
        let filter: BackupHistoryFilter = serde_json::from_value(json).unwrap();
        assert!(filter.config_id.is_none());
    }

    #[test]
    fn test_backup_config_with_storage_serialization() {
        let now = chrono::Utc::now();
        let config = BackupConfig {
            id: Uuid::new_v4(),
            name: "Daily Backup".to_string(),
            storage_id: Uuid::new_v4(),
            storage_path: "backups/daily".to_string(),
            schedule_cron: "0 0 2 * * * *".to_string(),
            retention_count: 7,
            enabled: true,
            created_at: now,
            updated_at: now,
        };
        let with_storage = BackupConfigWithStorage {
            config,
            storage_name: Some("S3 Primary".to_string()),
        };
        let json = serde_json::to_value(&with_storage).unwrap();
        assert_eq!(json["name"], "Daily Backup");
        assert_eq!(json["storage_name"], "S3 Primary");
        assert_eq!(json["schedule_cron"], "0 0 2 * * * *");
        assert_eq!(json["retention_count"], 7);
        assert!(json["enabled"].as_bool().unwrap());
    }

    #[test]
    fn test_backup_config_with_storage_null_name() {
        let now = chrono::Utc::now();
        let config = BackupConfig {
            id: Uuid::new_v4(),
            name: "Orphan Config".to_string(),
            storage_id: Uuid::new_v4(),
            storage_path: "".to_string(),
            schedule_cron: "0 0 2 * * * *".to_string(),
            retention_count: 3,
            enabled: false,
            created_at: now,
            updated_at: now,
        };
        let with_storage = BackupConfigWithStorage {
            config,
            storage_name: None,
        };
        let json = serde_json::to_value(&with_storage).unwrap();
        assert_eq!(json["name"], "Orphan Config");
        assert!(json["storage_name"].is_null());
    }

    // ─── Auth enforcement tests ───────────────────────────────────────────────

    use crate::config::AuthConfig;
    use crate::services::auth_service::AuthService;

    fn test_auth_service() -> AuthService {
        AuthService::new(&AuthConfig {
            jwt_secret: "test-secret-for-backup-endpoints".to_string(),
            access_token_ttl_secs: 900,
            refresh_token_ttl_secs: 604800,
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin123".to_string(),
        })
    }

    #[actix_rt::test]
    async fn test_list_backup_configs_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backup-configs", web::get().to(list_backup_configs)),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/backup-configs")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_list_backup_configs_requires_admin() {
        let auth_service = test_auth_service();
        let user_id = Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backup-configs", web::get().to(list_backup_configs)),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/backup-configs")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[actix_rt::test]
    async fn test_create_backup_config_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backup-configs", web::post().to(create_backup_config)),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/backup-configs")
            .set_json(serde_json::json!({
                "name": "Test",
                "storage_id": Uuid::new_v4(),
                "schedule_cron": "0 0 2 * * * *"
            }))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_create_backup_config_requires_admin() {
        let auth_service = test_auth_service();
        let user_id = Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backup-configs", web::post().to(create_backup_config)),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/backup-configs")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(serde_json::json!({
                "name": "Test",
                "storage_id": Uuid::new_v4(),
                "schedule_cron": "0 0 2 * * * *"
            }))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[actix_rt::test]
    async fn test_update_backup_config_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backup-configs/{id}", web::put().to(update_backup_config)),
        )
        .await;

        let id = Uuid::new_v4();
        let req = actix_web::test::TestRequest::put()
            .uri(&format!("/backup-configs/{}", id))
            .set_json(serde_json::json!({"name": "Updated"}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_update_backup_config_requires_admin() {
        let auth_service = test_auth_service();
        let user_id = Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backup-configs/{id}", web::put().to(update_backup_config)),
        )
        .await;

        let id = Uuid::new_v4();
        let req = actix_web::test::TestRequest::put()
            .uri(&format!("/backup-configs/{}", id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(serde_json::json!({"name": "Updated"}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[actix_rt::test]
    async fn test_delete_backup_config_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route(
                    "/backup-configs/{id}",
                    web::delete().to(delete_backup_config),
                ),
        )
        .await;

        let id = Uuid::new_v4();
        let req = actix_web::test::TestRequest::delete()
            .uri(&format!("/backup-configs/{}", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_delete_backup_config_requires_admin() {
        let auth_service = test_auth_service();
        let user_id = Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route(
                    "/backup-configs/{id}",
                    web::delete().to(delete_backup_config),
                ),
        )
        .await;

        let id = Uuid::new_v4();
        let req = actix_web::test::TestRequest::delete()
            .uri(&format!("/backup-configs/{}", id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[actix_rt::test]
    async fn test_list_backups_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backups", web::get().to(list_backups)),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/backups")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_list_backups_requires_admin() {
        let auth_service = test_auth_service();
        let user_id = Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backups", web::get().to(list_backups)),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/backups")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[actix_rt::test]
    async fn test_trigger_backup_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backups/trigger", web::post().to(trigger_backup)),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/backups/trigger")
            .set_json(serde_json::json!({"storage_id": Uuid::new_v4()}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_trigger_backup_requires_admin() {
        let auth_service = test_auth_service();
        let user_id = Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backups/trigger", web::post().to(trigger_backup)),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/backups/trigger")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(serde_json::json!({"storage_id": Uuid::new_v4()}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[actix_rt::test]
    async fn test_delete_backup_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backups/{id}", web::delete().to(delete_backup)),
        )
        .await;

        let id = Uuid::new_v4();
        let req = actix_web::test::TestRequest::delete()
            .uri(&format!("/backups/{}", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_delete_backup_requires_admin() {
        let auth_service = test_auth_service();
        let user_id = Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(web::Data::new(auth_service))
                .route("/backups/{id}", web::delete().to(delete_backup)),
        )
        .await;

        let id = Uuid::new_v4();
        let req = actix_web::test::TestRequest::delete()
            .uri(&format!("/backups/{}", id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }
}
