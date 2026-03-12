use actix_multipart::Multipart;
use actix_web::{web, HttpResponse};
use bytes::BytesMut;
use futures::TryStreamExt;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::db::models::{File, FileLocation, FileReference};
use crate::error::AppError;
use crate::services::{FileService, TierService};
use crate::storage::StorageRegistry;

use super::PaginationParams;

type HmacSha256 = Hmac<Sha256>;

// ─── Request/response types ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FileMetadata {
    pub file: File,
    pub locations: Vec<FileLocation>,
    pub references: Vec<FileReference>,
}

#[derive(Debug, Deserialize)]
pub struct TempLinkQuery {
    pub expires_in: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct TempLinkResponse {
    pub url: String,
    pub expires_in_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct DeleteFileQuery {
    pub project_id: Uuid,
}

// ─── Handlers ───────────────────────────────────────────────────────────────────

async fn upload_file(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    file_service: web::Data<FileService>,
    mut payload: Multipart,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();

    // Verify project exists
    crate::db::models::Project::find_by_id(pool.get_ref(), project_id).await?;

    let mut field = payload
        .try_next()
        .await
        .map_err(|e| AppError::BadRequest(format!("Multipart error: {}", e)))?
        .ok_or_else(|| AppError::BadRequest("No file provided".to_string()))?;

    let content_type = field
        .content_type()
        .map(|ct| ct.to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let filename = field
        .content_disposition()
        .and_then(|cd| cd.get_filename().map(|s| s.to_string()))
        .unwrap_or_else(|| "unnamed".to_string());

    let mut data = BytesMut::new();
    while let Some(chunk) = field
        .try_next()
        .await
        .map_err(|e| AppError::BadRequest(format!("Multipart read error: {}", e)))?
    {
        data.extend_from_slice(&chunk);
    }

    let result = file_service
        .upload_file(project_id, &filename, &content_type, data.freeze())
        .await?;

    Ok(HttpResponse::Created().json(result))
}

async fn list_project_files(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    query: web::Query<PaginationParams>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();

    // Verify project exists
    crate::db::models::Project::find_by_id(pool.get_ref(), project_id).await?;

    let refs =
        FileReference::list_for_project(pool.get_ref(), project_id, query.limit(), query.offset())
            .await?;
    Ok(HttpResponse::Ok().json(refs))
}

async fn get_file_metadata(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let file_id = path.into_inner();
    let file = File::find_by_id(pool.get_ref(), file_id).await?;
    let locations = FileLocation::find_all_for_file(pool.get_ref(), file_id).await?;

    let references = sqlx::query_as::<_, FileReference>(
        "SELECT * FROM file_references WHERE file_id = $1 ORDER BY created_at DESC",
    )
    .bind(file_id)
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(FileMetadata {
        file,
        locations,
        references,
    }))
}

async fn download_file(
    file_service: web::Data<FileService>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let file_id = path.into_inner();
    let result = file_service.download_file(file_id).await?;

    Ok(HttpResponse::Ok()
        .content_type(result.content_type)
        .body(result.data))
}

async fn get_temp_link(
    file_service: web::Data<FileService>,
    path: web::Path<Uuid>,
    query: web::Query<TempLinkQuery>,
) -> Result<HttpResponse, AppError> {
    let file_id = path.into_inner();
    let expires_in_secs = query.expires_in.unwrap_or(3600);
    let expires_in = std::time::Duration::from_secs(expires_in_secs);

    let url = file_service.generate_temp_link(file_id, expires_in).await?;

    Ok(HttpResponse::Ok().json(TempLinkResponse {
        url,
        expires_in_seconds: expires_in_secs,
    }))
}

async fn delete_file_reference(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    query: web::Query<DeleteFileQuery>,
) -> Result<HttpResponse, AppError> {
    let file_id = path.into_inner();
    FileReference::delete_by_file_and_project(pool.get_ref(), file_id, query.project_id).await?;
    Ok(HttpResponse::NoContent().finish())
}

async fn restore_file(
    tier_service: web::Data<TierService>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let file_id = path.into_inner();
    let task = tier_service.restore_file(file_id).await?;
    Ok(HttpResponse::Accepted().json(task))
}

// ─── Local temp URL download ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LocalTempDownloadQuery {
    pub path: String,
    pub expires: u64,
    pub sig: String,
}

/// GET /download/local - Serve files via HMAC-signed temporary URLs for local storage.
pub async fn download_local_temp(
    pool: web::Data<PgPool>,
    registry: web::Data<Arc<StorageRegistry>>,
    config: web::Data<AppConfig>,
    query: web::Query<LocalTempDownloadQuery>,
) -> Result<HttpResponse, AppError> {
    // Check expiration
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now > query.expires {
        return Err(AppError::Unauthorized(
            "Download link has expired".to_string(),
        ));
    }

    // Verify HMAC signature
    let message = format!("{}:{}", query.path, query.expires);
    let mut mac = HmacSha256::new_from_slice(config.storage.hmac_secret.as_bytes())
        .expect("HMAC accepts any key");
    mac.update(message.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    if !crate::constant_time_eq(expected.as_bytes(), query.sig.as_bytes()) {
        return Err(AppError::Unauthorized(
            "Invalid download link signature".to_string(),
        ));
    }

    // Find a synced file location matching this storage_path
    let location = sqlx::query_as::<_, FileLocation>(
        r#"SELECT fl.* FROM file_locations fl
           JOIN storages s ON s.id = fl.storage_id AND s.enabled = TRUE
           WHERE fl.storage_path = $1 AND fl.status = 'synced'
           LIMIT 1"#,
    )
    .bind(&query.path)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("File not found".to_string()))?;

    // Download from the storage backend
    let backend = registry.get(&location.storage_id).await?;
    let data = backend.download(&query.path).await?;

    // Get content type from the file record
    let file = File::find_by_id(pool.get_ref(), location.file_id).await?;

    // Update last_accessed_at
    let _ = FileLocation::touch_accessed(pool.get_ref(), location.id).await;

    Ok(HttpResponse::Ok()
        .content_type(file.content_type.as_str())
        .body(data))
}

// ─── Route configuration ────────────────────────────────────────────────────────

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/projects/{project_id}/files")
            .route(web::post().to(upload_file))
            .route(web::get().to(list_project_files)),
    )
    .service(
        web::resource("/files/{id}")
            .route(web::get().to(get_file_metadata))
            .route(web::delete().to(delete_file_reference)),
    )
    .service(web::resource("/files/{id}/download").route(web::get().to(download_file)))
    .service(web::resource("/files/{id}/link").route(web::get().to(get_temp_link)))
    .service(web::resource("/files/{id}/restore").route(web::post().to(restore_file)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temp_link_query_defaults() {
        let json = serde_json::json!({});
        let query: TempLinkQuery = serde_json::from_value(json).unwrap();
        assert!(query.expires_in.is_none());
    }

    #[test]
    fn test_temp_link_query_custom() {
        let json = serde_json::json!({"expires_in": 7200});
        let query: TempLinkQuery = serde_json::from_value(json).unwrap();
        assert_eq!(query.expires_in, Some(7200));
    }

    #[test]
    fn test_temp_link_response_serialization() {
        let resp = TempLinkResponse {
            url: "/download/local?path=abc&expires=123&sig=xyz".to_string(),
            expires_in_seconds: 3600,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["url"].as_str().unwrap().contains("/download/local"));
        assert_eq!(json["expires_in_seconds"], 3600);
    }

    #[test]
    fn test_file_metadata_serialization() {
        let now = chrono::Utc::now();
        let file_id = uuid::Uuid::new_v4();
        let metadata = FileMetadata {
            file: File {
                id: file_id,
                hash_sha256: "a".repeat(64),
                size: 1024,
                content_type: "text/plain".to_string(),
                created_at: now,
            },
            locations: vec![FileLocation {
                id: uuid::Uuid::new_v4(),
                file_id,
                storage_id: uuid::Uuid::new_v4(),
                storage_path: "ab/cd/abcdef".to_string(),
                status: "synced".to_string(),
                synced_at: Some(now),
                last_accessed_at: None,
                created_at: now,
            }],
            references: vec![FileReference {
                id: uuid::Uuid::new_v4(),
                file_id,
                project_id: uuid::Uuid::new_v4(),
                original_name: "test.txt".to_string(),
                created_at: now,
            }],
        };
        let json = serde_json::to_value(&metadata).unwrap();
        assert_eq!(json["file"]["size"], 1024);
        assert_eq!(json["locations"].as_array().unwrap().len(), 1);
        assert_eq!(json["references"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_delete_file_query_deserialization() {
        let json = serde_json::json!({
            "project_id": "550e8400-e29b-41d4-a716-446655440000"
        });
        let query: DeleteFileQuery = serde_json::from_value(json).unwrap();
        assert_eq!(
            query.project_id.to_string(),
            "550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_delete_file_query_missing_project_id() {
        let json = serde_json::json!({});
        let result: Result<DeleteFileQuery, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }
}
