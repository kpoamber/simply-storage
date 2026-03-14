use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::auth::AdminUser;
use crate::error::AppError;
use crate::services::BulkService;

// ─── Response types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ExportStarted {
    pub job_id: Uuid,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ExportStatusQuery {
    pub job_id: Uuid,
}

// ─── Handlers ───────────────────────────────────────────────────────────────────

/// POST /api/storages/{id}/sync-all
/// Enumerate all files not yet on this storage, create sync_tasks for each.
async fn sync_all(
    _admin: AdminUser,
    bulk_service: web::Data<BulkService>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let storage_id = path.into_inner();
    let result = bulk_service.sync_all(storage_id).await?;
    Ok(HttpResponse::Ok().json(result))
}

/// POST /api/storages/{id}/export
/// Start background job to produce tar.gz archive of all files on the storage.
async fn start_export(
    _admin: AdminUser,
    bulk_service: web::Data<BulkService>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let storage_id = path.into_inner();
    let job_id = bulk_service.start_export(storage_id).await?;
    Ok(HttpResponse::Accepted().json(ExportStarted {
        job_id,
        message: "Export started".to_string(),
    }))
}

/// GET /api/storages/{id}/export/status
/// Poll export job progress (percentage, file count).
async fn export_status(
    _admin: AdminUser,
    bulk_service: web::Data<BulkService>,
    query: web::Query<ExportStatusQuery>,
) -> Result<HttpResponse, AppError> {
    let status = bulk_service.get_export_status(query.job_id).await?;
    Ok(HttpResponse::Ok().json(status))
}

/// GET /api/storages/{id}/export/download
/// Stream completed archive.
async fn export_download(
    _admin: AdminUser,
    bulk_service: web::Data<BulkService>,
    query: web::Query<ExportStatusQuery>,
) -> Result<HttpResponse, AppError> {
    let data = bulk_service.get_export_data(query.job_id).await?;
    Ok(HttpResponse::Ok()
        .content_type("application/gzip")
        .append_header(("Content-Disposition", "attachment; filename=\"export.tar.gz\""))
        .body(data))
}

// ─── Route configuration ────────────────────────────────────────────────────────

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/storages/{id}/sync-all").route(web::post().to(sync_all)),
    )
    .service(
        web::resource("/storages/{id}/export").route(web::post().to(start_export)),
    )
    .service(
        web::resource("/storages/{id}/export/status").route(web::get().to(export_status)),
    )
    .service(
        web::resource("/storages/{id}/export/download").route(web::get().to(export_download)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_started_serialization() {
        let resp = ExportStarted {
            job_id: Uuid::new_v4(),
            message: "Export started".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["job_id"].is_string());
        assert_eq!(json["message"], "Export started");
    }

    #[test]
    fn test_export_status_query_deserialization() {
        let json = serde_json::json!({
            "job_id": "550e8400-e29b-41d4-a716-446655440000"
        });
        let query: ExportStatusQuery = serde_json::from_value(json).unwrap();
        assert_eq!(
            query.job_id.to_string(),
            "550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_export_status_query_missing_job_id() {
        let json = serde_json::json!({});
        let result: Result<ExportStatusQuery, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    // ─── Auth enforcement tests ───────────────────────────────────────────────

    use crate::config::AuthConfig;
    use crate::services::auth_service::AuthService;

    fn test_auth_service() -> AuthService {
        AuthService::new(&AuthConfig {
            jwt_secret: "test-secret-for-bulk-endpoints".to_string(),
            access_token_ttl_secs: 900,
            refresh_token_ttl_secs: 604800,
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin123".to_string(),
        })
    }

    #[actix_rt::test]
    async fn test_sync_all_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route("/storages/{id}/sync-all", actix_web::web::post().to(sync_all)),
        )
        .await;

        let id = Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/storages/{}/sync-all", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_sync_all_requires_admin() {
        let auth_service = test_auth_service();
        let user_id = Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route("/storages/{id}/sync-all", actix_web::web::post().to(sync_all)),
        )
        .await;

        let id = Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/storages/{}/sync-all", id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[actix_rt::test]
    async fn test_start_export_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route("/storages/{id}/export", actix_web::web::post().to(start_export)),
        )
        .await;

        let id = Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/storages/{}/export", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_start_export_requires_admin() {
        let auth_service = test_auth_service();
        let user_id = Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route("/storages/{id}/export", actix_web::web::post().to(start_export)),
        )
        .await;

        let id = Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/storages/{}/export", id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }
}
