use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tokio_util::task::TaskTracker;
use uuid::Uuid;

use super::auth::AdminUser;
use crate::db::models::{
    BackupConfig, BackupRecord, CreateBackupConfig, CreateBackupRecord, Storage, UpdateBackupConfig,
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

#[derive(Debug, Serialize)]
pub struct BackupHistoryResponse {
    pub records: Vec<BackupRecord>,
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
    pub total_pages: i64,
}

#[derive(Debug, Deserialize)]
pub struct DeleteConfigQuery {
    pub delete_backups: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct BackupHistoryFilter {
    pub config_id: Option<Uuid>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
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
    // Validate field lengths against DB column limits
    if body.name.len() > 255 {
        return Err(AppError::BadRequest("name must be at most 255 characters".to_string()));
    }
    if body.schedule_cron.len() > 100 {
        return Err(AppError::BadRequest("schedule_cron must be at most 100 characters".to_string()));
    }
    if let Some(ref path) = body.storage_path {
        if path.len() > 1024 {
            return Err(AppError::BadRequest("storage_path must be at most 1024 characters".to_string()));
        }
    }

    // Validate cron expression
    BackupService::validate_cron(&body.schedule_cron)?;

    // Validate storage_path if provided
    if let Some(ref path) = body.storage_path {
        BackupService::validate_storage_path(path)?;
    }

    // Validate retention_count if provided
    if let Some(count) = body.retention_count {
        if count < 1 {
            return Err(AppError::BadRequest(
                "retention_count must be at least 1".to_string(),
            ));
        }
    }

    // Validate storage exists and is enabled
    let storage = Storage::find_by_id(pool.get_ref(), body.storage_id).await?;
    if !storage.enabled {
        return Err(AppError::BadRequest(
            "Storage is disabled. Enable it before assigning to a backup config.".to_string(),
        ));
    }

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

    // Validate field lengths against DB column limits
    if let Some(ref name) = body.name {
        if name.len() > 255 {
            return Err(AppError::BadRequest("name must be at most 255 characters".to_string()));
        }
    }
    if let Some(ref cron_expr) = body.schedule_cron {
        if cron_expr.len() > 100 {
            return Err(AppError::BadRequest("schedule_cron must be at most 100 characters".to_string()));
        }
    }
    if let Some(ref path) = body.storage_path {
        if path.len() > 1024 {
            return Err(AppError::BadRequest("storage_path must be at most 1024 characters".to_string()));
        }
    }

    // Validate cron expression if provided
    if let Some(ref cron_expr) = body.schedule_cron {
        BackupService::validate_cron(cron_expr)?;
    }

    // Validate storage_path if provided
    if let Some(ref path) = body.storage_path {
        BackupService::validate_storage_path(path)?;
    }

    // Validate retention_count if provided
    if let Some(count) = body.retention_count {
        if count < 1 {
            return Err(AppError::BadRequest(
                "retention_count must be at least 1".to_string(),
            ));
        }
    }

    // Validate storage exists and is enabled if provided
    if let Some(storage_id) = body.storage_id {
        let storage = Storage::find_by_id(pool.get_ref(), storage_id).await?;
        if !storage.enabled {
            return Err(AppError::BadRequest(
                "Storage is disabled. Enable it before assigning to a backup config.".to_string(),
            ));
        }
    }

    let config = BackupConfig::update(pool.get_ref(), config_id, &body).await?;
    Ok(HttpResponse::Ok().json(config))
}

async fn delete_backup_config(
    _admin: AdminUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    query: web::Query<DeleteConfigQuery>,
    service: web::Data<Arc<BackupService>>,
) -> Result<HttpResponse, AppError> {
    let config_id = path.into_inner();

    // Verify config exists before attempting cleanup
    BackupConfig::find_by_id(pool.get_ref(), config_id).await?;

    // If delete_backups is true, remove associated backup files and records
    // before deleting the config (while config_id FK is still intact).
    if query.delete_backups.unwrap_or(false) {
        let deleted = service.delete_all_backups_for_config(config_id).await?;
        if deleted > 0 {
            tracing::info!(
                config_id = %config_id,
                deleted_count = deleted,
                "Deleted associated backups before config removal"
            );
        }
    }

    BackupConfig::delete(pool.get_ref(), config_id).await?;
    Ok(HttpResponse::NoContent().finish())
}

// ─── Backup history handlers ────────────────────────────────────────────────────

async fn list_backups(
    _admin: AdminUser,
    pool: web::Data<PgPool>,
    query: web::Query<BackupHistoryFilter>,
) -> Result<HttpResponse, AppError> {
    let limit = query.per_page.unwrap_or(50).clamp(1, 500);
    let page = query.page.unwrap_or(1).max(1);
    let offset = page.saturating_sub(1).saturating_mul(limit);
    let total = BackupRecord::count(pool.get_ref(), query.config_id).await?;
    let records = BackupRecord::list(pool.get_ref(), query.config_id, limit, offset).await?;
    let total_pages = (total + limit - 1) / limit;
    Ok(HttpResponse::Ok().json(BackupHistoryResponse {
        records,
        page,
        per_page: limit,
        total,
        total_pages,
    }))
}

async fn trigger_backup(
    _admin: AdminUser,
    pool: web::Data<PgPool>,
    service: web::Data<Arc<BackupService>>,
    tracker: web::Data<TaskTracker>,
    body: web::Json<TriggerBackupRequest>,
) -> Result<HttpResponse, AppError> {
    let (config_id, config_name, storage_id, storage_path) = if let Some(cfg_id) = body.config_id {
        // Use config settings
        let config = BackupConfig::find_by_id(pool.get_ref(), cfg_id).await?;
        (Some(cfg_id), Some(config.name), config.storage_id, config.storage_path)
    } else if let Some(sid) = body.storage_id {
        // Manual backup with explicit storage
        Storage::find_by_id(pool.get_ref(), sid).await?;
        let path = body.storage_path.clone().unwrap_or_default();
        (None, None, sid, path)
    } else {
        return Err(AppError::BadRequest(
            "Either config_id or storage_id must be provided".to_string(),
        ));
    };

    // Validate storage_path
    BackupService::validate_storage_path(&storage_path)?;

    // Validate storage backend is registered and available before creating
    // the DB record, so we return a synchronous error instead of creating
    // a doomed "running" record that fails asynchronously.
    service.validate_storage_available(&storage_id).await?;

    // Clean up stale "running" records (older than 1 hour) so they don't
    // permanently block manual retriggers after a crash.
    if let Some(cfg_id) = config_id {
        let cleaned = BackupRecord::mark_stale_running_as_failed(pool.get_ref(), cfg_id, 3600).await?;
        if cleaned > 0 {
            tracing::warn!(config_id = %cfg_id, cleaned, "Cleaned up stale running backup records");
        }
    }

    // Prevent overlapping backups for the same config
    if let Some(cfg_id) = config_id {
        if BackupRecord::has_running_by_config(pool.get_ref(), cfg_id).await? {
            return Err(AppError::BadRequest(
                "A backup is already running for this config".to_string(),
            ));
        }
    }

    // Create the record immediately in "running" status
    let filename = BackupService::generate_backup_filename();
    let full_path = BackupService::build_upload_path(&storage_path, &filename);

    let record = BackupRecord::create(
        pool.get_ref(),
        &CreateBackupRecord {
            config_id,
            config_name,
            storage_id,
            file_path: full_path.clone(),
        },
    )
    .await?;

    // Spawn the actual backup work in background so the HTTP request returns immediately.
    // Use the TaskTracker so graceful shutdown waits for in-flight backups.
    let service = service.get_ref().clone();
    let record_id = record.id;
    tracker.spawn(async move {
        // For config-based triggers, acquire the same advisory lock the scheduler
        // uses. This prevents a race where the scheduler has acquired the lock
        // but hasn't yet inserted its "running" record, allowing a duplicate
        // pg_dump to start.
        let lock_conn = if let Some(cfg_id) = config_id {
            let id_bytes = cfg_id.as_bytes();
            let key1 = i32::from_le_bytes([id_bytes[0], id_bytes[1], id_bytes[2], id_bytes[3]]);
            let key2 = i32::from_le_bytes([id_bytes[4], id_bytes[5], id_bytes[6], id_bytes[7]]);

            match service.pool().acquire().await {
                Ok(mut conn) => {
                    let locked: Result<(bool,), _> =
                        sqlx::query_as("SELECT pg_try_advisory_lock($1, $2)")
                            .bind(key1)
                            .bind(key2)
                            .fetch_one(&mut *conn)
                            .await;
                    match locked {
                        Ok((true,)) => Some((conn, key1, key2)),
                        Ok((false,)) => {
                            let _ = BackupRecord::mark_failed(
                                service.pool(),
                                record_id,
                                "Skipped: another backup for this config is already in progress",
                            )
                            .await;
                            tracing::warn!(backup_id = %record_id, config_id = %cfg_id, "Manual backup skipped: advisory lock held by scheduler");
                            return;
                        }
                        Err(e) => {
                            let _ = BackupRecord::mark_failed(
                                service.pool(),
                                record_id,
                                &format!("Failed to acquire advisory lock: {}", e),
                            )
                            .await;
                            tracing::error!(backup_id = %record_id, error = %e, "Failed to acquire advisory lock, aborting backup");
                            return;
                        }
                    }
                }
                Err(e) => {
                    let _ = BackupRecord::mark_failed(
                        service.pool(),
                        record_id,
                        &format!("Failed to acquire connection for advisory lock: {}", e),
                    )
                    .await;
                    tracing::error!(backup_id = %record_id, error = %e, "Failed to acquire connection for advisory lock, aborting backup");
                    return;
                }
            }
        } else {
            None
        };

        match service.execute_backup_and_update(record_id, &full_path, storage_id).await {
            Ok(_) => {
                tracing::info!(backup_id = %record_id, "Manual backup completed");
                // Run retention cleanup if triggered from a config
                if let Some(cfg_id) = config_id {
                    if let Ok(config) = BackupConfig::find_by_id(service.pool(), cfg_id).await {
                        if let Err(e) = service.cleanup_old_backups(cfg_id, config.retention_count).await {
                            tracing::error!(config_id = %cfg_id, error = %e, "Failed retention cleanup after manual backup");
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(backup_id = %record_id, error = %e, "Manual backup failed");
            }
        }

        // Release advisory lock on the same connection that acquired it
        if let Some((mut conn, key1, key2)) = lock_conn {
            let _ = sqlx::query("SELECT pg_advisory_unlock($1, $2)")
                .bind(key1)
                .bind(key2)
                .execute(&mut *conn)
                .await;
        }
    });

    Ok(HttpResponse::Created().json(record))
}

async fn delete_backup(
    _admin: AdminUser,
    path: web::Path<Uuid>,
    service: web::Data<Arc<BackupService>>,
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
