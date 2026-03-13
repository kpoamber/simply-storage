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
use crate::db::models::{
    BulkDeleteFilters, File, FileLocation, FileReference, MetadataFilter, Project, Storage,
    UserProject,
};
use crate::error::AppError;
use crate::services::{FileService, TierService};
use crate::storage::StorageRegistry;

use super::auth::AuthenticatedUser;
use super::PaginationParams;

type HmacSha256 = Hmac<Sha256>;

// ─── Request/response types ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FileMetadata {
    pub file: File,
    pub locations: Vec<FileLocation>,
    pub references: Vec<FileReference>,
}

#[derive(Debug, Serialize)]
pub struct FileReferenceWithSync {
    #[serde(flatten)]
    pub file_ref: FileReference,
    pub sync_status: String,
    pub synced_storages: i64,
    pub total_storages: i64,
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

// ─── Auth helper ─────────────────────────────────────────────────────────────────

/// Check that the user has access to at least one project referencing this file.
async fn check_file_access(
    pool: &PgPool,
    file_id: Uuid,
    user: &AuthenticatedUser,
) -> Result<(), AppError> {
    if user.role == "admin" {
        return Ok(());
    }
    let row: (bool,) = sqlx::query_as(
        r#"SELECT EXISTS(
            SELECT 1 FROM file_references fr
            JOIN projects p ON p.id = fr.project_id
            WHERE fr.file_id = $1 AND p.deleted_at IS NULL
            AND (p.owner_id = $2 OR EXISTS (
                SELECT 1 FROM user_projects up WHERE up.project_id = p.id AND up.user_id = $2
            ))
        )"#,
    )
    .bind(file_id)
    .bind(user.user_id)
    .fetch_one(pool)
    .await?;

    if !row.0 {
        return Err(AppError::Forbidden(
            "Access denied: not a member".to_string(),
        ));
    }
    Ok(())
}

/// Check that the user has write access (owner or admin) to at least one project referencing this file.
async fn check_file_write_access(
    pool: &PgPool,
    file_id: Uuid,
    user: &AuthenticatedUser,
) -> Result<(), AppError> {
    if user.role == "admin" {
        return Ok(());
    }
    let row: (bool,) = sqlx::query_as(
        r#"SELECT EXISTS(
            SELECT 1 FROM file_references fr
            JOIN projects p ON p.id = fr.project_id
            WHERE fr.file_id = $1 AND p.deleted_at IS NULL
            AND p.owner_id = $2
        )"#,
    )
    .bind(file_id)
    .bind(user.user_id)
    .fetch_one(pool)
    .await?;

    if !row.0 {
        return Err(AppError::Forbidden(
            "Access denied: owner or admin required".to_string(),
        ));
    }
    Ok(())
}

// ─── Handlers ───────────────────────────────────────────────────────────────────

/// Validate that metadata is a flat JSON object: keys are strings, values are strings/numbers/booleans.
/// Rejects nested objects and arrays.
fn validate_flat_metadata(value: &serde_json::Value) -> Result<(), AppError> {
    let obj = value.as_object().ok_or_else(|| {
        AppError::BadRequest("Metadata must be a JSON object".to_string())
    })?;
    for (key, val) in obj {
        match val {
            serde_json::Value::String(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::Bool(_)
            | serde_json::Value::Null => {}
            _ => {
                return Err(AppError::BadRequest(format!(
                    "Metadata value for key '{}' must be a string, number, boolean, or null; nested objects and arrays are not allowed",
                    key
                )));
            }
        }
    }
    Ok(())
}

async fn upload_file(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
    file_service: web::Data<FileService>,
    mut payload: Multipart,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();

    // Verify project exists and user has access
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    user.require_owner_or_admin(project.owner_id)?;

    let mut file_data: Option<(String, String, BytesMut)> = None;
    let mut metadata = serde_json::json!({});

    while let Some(mut field) = payload
        .try_next()
        .await
        .map_err(|e| AppError::BadRequest(format!("Multipart error: {}", e)))?
    {
        let field_name = field
            .content_disposition()
            .and_then(|cd| cd.get_name().map(|s| s.to_string()))
            .unwrap_or_default();

        match field_name.as_str() {
            "metadata" => {
                const MAX_METADATA_SIZE: usize = 1_048_576; // 1 MB
                let mut buf = BytesMut::new();
                while let Some(chunk) = field
                    .try_next()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("Multipart read error: {}", e)))?
                {
                    buf.extend_from_slice(&chunk);
                    if buf.len() > MAX_METADATA_SIZE {
                        return Err(AppError::BadRequest(
                            "Metadata field too large (max 1 MB)".to_string(),
                        ));
                    }
                }
                let meta_str = std::str::from_utf8(&buf)
                    .map_err(|_| AppError::BadRequest("Metadata must be valid UTF-8".to_string()))?;
                metadata = serde_json::from_str(meta_str)
                    .map_err(|e| AppError::BadRequest(format!("Invalid metadata JSON: {}", e)))?;
                validate_flat_metadata(&metadata)?;
            }
            _ => {
                // Treat as file field (first file field wins)
                if file_data.is_none() {
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
                    file_data = Some((filename, content_type, data));
                }
            }
        }
    }

    let (filename, content_type, data) =
        file_data.ok_or_else(|| AppError::BadRequest("No file provided".to_string()))?;

    let result = file_service
        .upload_file(project_id, &filename, &content_type, data.freeze(), metadata)
        .await?;

    Ok(HttpResponse::Created().json(result))
}

async fn list_project_files(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
    query: web::Query<PaginationParams>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();

    // Verify project exists and user has read access (admin, owner, or member)
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    if !user.is_admin() && !user.is_owner(project.owner_id) {
        let is_member = UserProject::is_member(pool.get_ref(), user.user_id, project_id).await?;
        if !is_member {
            return Err(AppError::Forbidden("Access denied: not a member".to_string()));
        }
    }

    let refs =
        FileReference::list_for_project(pool.get_ref(), project_id, query.limit(), query.offset())
            .await?;

    // Count total enabled storages available for this project
    let total_storages = Storage::list_for_project(pool.get_ref(), project_id)
        .await?
        .len() as i64;

    // Enrich each file reference with sync status
    let mut result = Vec::with_capacity(refs.len());
    for file_ref in refs {
        let synced_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM file_locations WHERE file_id = $1 AND status = 'synced'",
        )
        .bind(file_ref.file_id)
        .fetch_one(pool.get_ref())
        .await
        .unwrap_or((0,));

        let sync_status = if total_storages == 0 {
            "no_storage".to_string()
        } else if synced_count.0 >= total_storages {
            "synced".to_string()
        } else if synced_count.0 > 0 {
            "partial".to_string()
        } else {
            "pending".to_string()
        };

        result.push(FileReferenceWithSync {
            file_ref,
            sync_status,
            synced_storages: synced_count.0,
            total_storages,
        });
    }

    Ok(HttpResponse::Ok().json(result))
}

async fn get_file_metadata(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let file_id = path.into_inner();
    check_file_access(pool.get_ref(), file_id, &user).await?;

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
    pool: web::Data<PgPool>,
    file_service: web::Data<FileService>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let file_id = path.into_inner();
    check_file_access(pool.get_ref(), file_id, &user).await?;

    let result = file_service.download_file(file_id).await?;

    let mut response = HttpResponse::Ok();
    response.content_type(result.content_type);
    if let Some(ref name) = result.original_name {
        // Sanitize filename to prevent header injection
        let safe_name: String = name
            .chars()
            .filter(|c| *c != '"' && *c != '\\' && *c != '\r' && *c != '\n')
            .collect();
        response.insert_header((
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", safe_name),
        ));
    }
    Ok(response.body(result.data))
}

async fn get_temp_link(
    pool: web::Data<PgPool>,
    file_service: web::Data<FileService>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
    query: web::Query<TempLinkQuery>,
) -> Result<HttpResponse, AppError> {
    let file_id = path.into_inner();
    check_file_access(pool.get_ref(), file_id, &user).await?;

    let expires_in_secs = query.expires_in.unwrap_or(3600).min(86400);
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
    user: AuthenticatedUser,
    query: web::Query<DeleteFileQuery>,
) -> Result<HttpResponse, AppError> {
    let file_id = path.into_inner();
    // Check ownership of the specific project being removed from
    let project = Project::find_by_id(pool.get_ref(), query.project_id).await?;
    user.require_owner_or_admin(project.owner_id)?;

    FileReference::delete_by_file_and_project(pool.get_ref(), file_id, query.project_id).await?;
    Ok(HttpResponse::NoContent().finish())
}

async fn restore_file(
    pool: web::Data<PgPool>,
    tier_service: web::Data<TierService>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let file_id = path.into_inner();
    check_file_write_access(pool.get_ref(), file_id, &user).await?;

    let task = tier_service.restore_file(file_id).await?;
    Ok(HttpResponse::Accepted().json(task))
}

// ─── Metadata search ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MetadataSearchRequest {
    pub filters: Option<MetadataFilter>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

async fn search_files(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
    body: web::Json<MetadataSearchRequest>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();

    // Verify project exists and user has read access (admin, owner, or member)
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    if !user.is_admin() && !user.is_owner(project.owner_id) {
        let is_member = UserProject::is_member(pool.get_ref(), user.user_id, project_id).await?;
        if !is_member {
            return Err(AppError::Forbidden("Access denied: not a member".to_string()));
        }
    }

    let page = body.page.unwrap_or(1);
    let per_page = body.per_page.unwrap_or(50);

    let result = FileReference::search_by_metadata(
        pool.get_ref(),
        project_id,
        body.filters.as_ref(),
        page,
        per_page,
    )
    .await?;

    Ok(HttpResponse::Ok().json(result))
}

// ─── Search summary ──────────────────────────────────────────────────────────

async fn search_summary(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
    body: web::Json<MetadataSearchRequest>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();

    // Verify project exists and user has read access (admin, owner, or member)
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    if !user.is_admin() && !user.is_owner(project.owner_id) {
        let is_member = UserProject::is_member(pool.get_ref(), user.user_id, project_id).await?;
        if !is_member {
            return Err(AppError::Forbidden("Access denied: not a member".to_string()));
        }
    }

    let summary = FileReference::search_summary(
        pool.get_ref(),
        project_id,
        body.filters.as_ref(),
    )
    .await?;

    Ok(HttpResponse::Ok().json(summary))
}

// ─── Bulk delete ──────────────────────────────────────────────────────────────

async fn bulk_delete_preview(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
    body: web::Json<BulkDeleteFilters>,
    file_service: web::Data<FileService>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();

    // Verify project exists and user is owner or admin
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    user.require_owner_or_admin(project.owner_id)?;

    let filters = body.into_inner();
    if !filters.has_any_filter() {
        return Err(AppError::BadRequest(
            "At least one filter is required for bulk delete".to_string(),
        ));
    }

    let preview = file_service.bulk_delete_preview(project_id, &filters).await?;
    Ok(HttpResponse::Ok().json(preview))
}

async fn bulk_delete(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
    body: web::Json<BulkDeleteFilters>,
    file_service: web::Data<FileService>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();

    // Verify project exists and user is owner or admin
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    user.require_owner_or_admin(project.owner_id)?;

    let filters = body.into_inner();
    if !filters.has_any_filter() {
        return Err(AppError::BadRequest(
            "At least one filter is required for bulk delete".to_string(),
        ));
    }

    let result = file_service.bulk_delete(project_id, &filters).await?;
    Ok(HttpResponse::Ok().json(result))
}

// ─── Local temp URL download ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LocalTempDownloadQuery {
    pub path: String,
    pub expires: u64,
    pub sig: String,
}

/// GET /download/local - Serve files via HMAC-signed temporary URLs for local storage.
/// No auth required - the HMAC signature serves as authentication.
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
             AND s.storage_type = 'local'
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
        web::resource("/projects/{project_id}/files/search")
            .route(web::post().to(search_files)),
    )
    .service(
        web::resource("/projects/{project_id}/files/search/summary")
            .route(web::post().to(search_summary)),
    )
    .service(
        web::resource("/projects/{project_id}/files/bulk-delete/preview")
            .route(web::post().to(bulk_delete_preview)),
    )
    .service(
        web::resource("/projects/{project_id}/files/bulk-delete")
            .route(web::post().to(bulk_delete)),
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
                metadata: serde_json::json!({}),
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

    // ─── Metadata validation tests ──────────────────────────────────────────

    #[test]
    fn test_validate_flat_metadata_empty_object() {
        let meta = serde_json::json!({});
        assert!(validate_flat_metadata(&meta).is_ok());
    }

    #[test]
    fn test_validate_flat_metadata_valid_types() {
        let meta = serde_json::json!({
            "env": "production",
            "version": 42,
            "active": true,
            "score": 3.14,
            "optional": null
        });
        assert!(validate_flat_metadata(&meta).is_ok());
    }

    #[test]
    fn test_validate_flat_metadata_rejects_nested_object() {
        let meta = serde_json::json!({
            "env": "prod",
            "nested": {"inner": "value"}
        });
        let err = validate_flat_metadata(&meta).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
        assert!(err.to_string().contains("nested"));
    }

    #[test]
    fn test_validate_flat_metadata_rejects_array() {
        let meta = serde_json::json!({
            "tags": ["a", "b", "c"]
        });
        let err = validate_flat_metadata(&meta).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
        assert!(err.to_string().contains("tags"));
    }

    #[test]
    fn test_validate_flat_metadata_rejects_non_object() {
        let meta = serde_json::json!("not an object");
        let err = validate_flat_metadata(&meta).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn test_validate_flat_metadata_rejects_array_root() {
        let meta = serde_json::json!([1, 2, 3]);
        let err = validate_flat_metadata(&meta).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn test_file_reference_with_sync_includes_metadata() {
        let now = chrono::Utc::now();
        let meta = serde_json::json!({"env": "staging", "version": 2});
        let ref_with_sync = FileReferenceWithSync {
            file_ref: FileReference {
                id: uuid::Uuid::new_v4(),
                file_id: uuid::Uuid::new_v4(),
                project_id: uuid::Uuid::new_v4(),
                original_name: "test.txt".to_string(),
                metadata: meta.clone(),
                created_at: now,
            },
            sync_status: "synced".to_string(),
            synced_storages: 2,
            total_storages: 2,
        };
        let json = serde_json::to_value(&ref_with_sync).unwrap();
        // metadata should be flattened into the top-level JSON
        assert_eq!(json["metadata"]["env"], "staging");
        assert_eq!(json["metadata"]["version"], 2);
        assert_eq!(json["sync_status"], "synced");
    }

    #[test]
    fn test_file_metadata_response_includes_reference_metadata() {
        let now = chrono::Utc::now();
        let file_id = uuid::Uuid::new_v4();
        let meta = serde_json::json!({"department": "engineering"});
        let file_meta = FileMetadata {
            file: File {
                id: file_id,
                hash_sha256: "a".repeat(64),
                size: 2048,
                content_type: "application/pdf".to_string(),
                created_at: now,
            },
            locations: vec![],
            references: vec![FileReference {
                id: uuid::Uuid::new_v4(),
                file_id,
                project_id: uuid::Uuid::new_v4(),
                original_name: "report.pdf".to_string(),
                metadata: meta.clone(),
                created_at: now,
            }],
        };
        let json = serde_json::to_value(&file_meta).unwrap();
        assert_eq!(json["references"][0]["metadata"]["department"], "engineering");
    }

    #[test]
    fn test_upload_result_includes_metadata() {
        let now = chrono::Utc::now();
        let meta = serde_json::json!({"env": "test", "priority": 1});
        let result = crate::services::file_service::UploadResult {
            file: File {
                id: uuid::Uuid::new_v4(),
                hash_sha256: "b".repeat(64),
                size: 512,
                content_type: "text/plain".to_string(),
                created_at: now,
            },
            file_reference: FileReference {
                id: uuid::Uuid::new_v4(),
                file_id: uuid::Uuid::new_v4(),
                project_id: uuid::Uuid::new_v4(),
                original_name: "data.txt".to_string(),
                metadata: meta.clone(),
                created_at: now,
            },
            is_duplicate: false,
            sync_tasks_created: 0,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["file_reference"]["metadata"]["env"], "test");
        assert_eq!(json["file_reference"]["metadata"]["priority"], 1);
    }

    // ─── Metadata search tests ─────────────────────────────────────────────

    #[test]
    fn test_metadata_search_request_defaults() {
        let json = serde_json::json!({});
        let req: MetadataSearchRequest = serde_json::from_value(json).unwrap();
        assert!(req.filters.is_none());
        assert!(req.page.is_none());
        assert!(req.per_page.is_none());
    }

    #[test]
    fn test_metadata_search_request_with_filters() {
        let json = serde_json::json!({
            "filters": {"key": "env", "value": "prod"},
            "page": 2,
            "per_page": 25
        });
        let req: MetadataSearchRequest = serde_json::from_value(json).unwrap();
        assert!(req.filters.is_some());
        assert_eq!(req.page, Some(2));
        assert_eq!(req.per_page, Some(25));
    }

    #[test]
    fn test_metadata_search_request_with_and_filter() {
        let json = serde_json::json!({
            "filters": {
                "and": [
                    {"key": "env", "value": "prod"},
                    {"not": {"key": "status", "value": "deprecated"}}
                ]
            }
        });
        let req: MetadataSearchRequest = serde_json::from_value(json).unwrap();
        assert!(req.filters.is_some());
        match req.filters.unwrap() {
            crate::db::models::MetadataFilter::And { and } => {
                assert_eq!(and.len(), 2);
            }
            _ => panic!("Expected And filter"),
        }
    }

    #[test]
    fn test_metadata_search_request_with_or_filter() {
        let json = serde_json::json!({
            "filters": {
                "or": [
                    {"key": "env", "value": "prod"},
                    {"key": "env", "value": "staging"}
                ]
            }
        });
        let req: MetadataSearchRequest = serde_json::from_value(json).unwrap();
        assert!(req.filters.is_some());
    }

    #[test]
    fn test_metadata_search_request_with_nested_filters() {
        let json = serde_json::json!({
            "filters": {
                "and": [
                    {"key": "env", "value": "prod"},
                    {"or": [
                        {"key": "tier", "value": "hot"},
                        {"key": "tier", "value": "warm"}
                    ]},
                    {"not": {"key": "archived", "value": true}}
                ]
            },
            "page": 1,
            "per_page": 10
        });
        let req: MetadataSearchRequest = serde_json::from_value(json).unwrap();
        assert!(req.filters.is_some());
    }

    #[test]
    fn test_metadata_search_empty_filters_returns_all() {
        // Empty filters (None) means no metadata filter => returns all
        let json = serde_json::json!({"filters": null});
        let req: MetadataSearchRequest = serde_json::from_value(json).unwrap();
        assert!(req.filters.is_none());
    }

    // ─── Search summary tests ─────────────────────────────────────────────

    #[test]
    fn test_search_summary_request_empty() {
        // Summary reuses MetadataSearchRequest; empty filters = summarize all
        let json = serde_json::json!({});
        let req: MetadataSearchRequest = serde_json::from_value(json).unwrap();
        assert!(req.filters.is_none());
    }

    #[test]
    fn test_search_summary_request_with_filters() {
        let json = serde_json::json!({
            "filters": {
                "and": [
                    {"key": "env", "value": "prod"},
                    {"key": "region", "value": "us-east"}
                ]
            }
        });
        let req: MetadataSearchRequest = serde_json::from_value(json).unwrap();
        assert!(req.filters.is_some());
        match req.filters.unwrap() {
            crate::db::models::MetadataFilter::And { and } => assert_eq!(and.len(), 2),
            _ => panic!("Expected And filter"),
        }
    }

    #[test]
    fn test_search_summary_response_shape() {
        use crate::db::models::{SearchSummary, TimelineEntry};
        let summary = SearchSummary {
            total_files: 15,
            total_size: 5242880,
            earliest_upload: Some(chrono::Utc::now()),
            latest_upload: Some(chrono::Utc::now()),
            timeline: vec![TimelineEntry {
                date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                count: 15,
                size: 5242880,
            }],
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["total_files"], 15);
        assert_eq!(json["total_size"], 5242880);
        assert!(!json["earliest_upload"].is_null());
        assert!(!json["latest_upload"].is_null());
        assert_eq!(json["timeline"].as_array().unwrap().len(), 1);
        assert_eq!(json["timeline"][0]["date"], "2026-03-01");
    }

    // ─── Bulk delete tests ─────────────────────────────────────────────────

    #[test]
    fn test_bulk_delete_filters_deserialization_with_all_fields() {
        let json = serde_json::json!({
            "metadata_filters": {
                "and": [
                    {"key": "env", "value": "prod"},
                    {"not": {"key": "status", "value": "deprecated"}}
                ]
            },
            "created_before": "2026-01-01T00:00:00Z",
            "created_after": "2025-01-01T00:00:00Z",
            "size_min": 1048576,
            "size_max": 10485760,
            "last_accessed_before": "2025-06-01T00:00:00Z"
        });
        let filters: crate::db::models::BulkDeleteFilters =
            serde_json::from_value(json).unwrap();
        assert!(filters.metadata_filters.is_some());
        assert!(filters.created_before.is_some());
        assert!(filters.created_after.is_some());
        assert_eq!(filters.size_min, Some(1048576));
        assert_eq!(filters.size_max, Some(10485760));
        assert!(filters.last_accessed_before.is_some());
    }

    #[test]
    fn test_bulk_delete_filters_empty_rejected() {
        let json = serde_json::json!({});
        let filters: crate::db::models::BulkDeleteFilters =
            serde_json::from_value(json).unwrap();
        assert!(!filters.has_any_filter());
    }

    #[test]
    fn test_bulk_delete_filters_metadata_only() {
        let json = serde_json::json!({
            "metadata_filters": {"key": "env", "value": "staging"}
        });
        let filters: crate::db::models::BulkDeleteFilters =
            serde_json::from_value(json).unwrap();
        assert!(filters.has_any_filter());
    }

    #[test]
    fn test_bulk_delete_filters_date_only() {
        let json = serde_json::json!({
            "created_before": "2026-06-01T00:00:00Z"
        });
        let filters: crate::db::models::BulkDeleteFilters =
            serde_json::from_value(json).unwrap();
        assert!(filters.has_any_filter());
        assert!(filters.created_before.is_some());
    }

    #[test]
    fn test_bulk_delete_preview_response_shape() {
        let preview = crate::db::models::BulkDeletePreview {
            matching_references: 25,
            total_size: 5242880,
        };
        let json = serde_json::to_value(&preview).unwrap();
        assert_eq!(json["matching_references"], 25);
        assert_eq!(json["total_size"], 5242880);
    }

    #[test]
    fn test_bulk_delete_result_response_shape() {
        let result = crate::db::models::BulkDeleteResult {
            deleted_references: 10,
            orphaned_files_cleaned: 3,
            freed_bytes: 1048576,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["deleted_references"], 10);
        assert_eq!(json["orphaned_files_cleaned"], 3);
        assert_eq!(json["freed_bytes"], 1048576);
    }

    // ─── Auth enforcement tests ───────────────────────────────────────────────

    use crate::config::AuthConfig;
    use crate::services::auth_service::AuthService;

    fn test_auth_service() -> AuthService {
        AuthService::new(&AuthConfig {
            jwt_secret: "test-secret-for-file-endpoints".to_string(),
            access_token_ttl_secs: 900,
            refresh_token_ttl_secs: 604800,
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin123".to_string(),
        })
    }

    #[actix_rt::test]
    async fn test_upload_file_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{project_id}/files",
                    actix_web::web::post().to(upload_file),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/projects/{}/files", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_list_project_files_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{project_id}/files",
                    actix_web::web::get().to(list_project_files),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::get()
            .uri(&format!("/projects/{}/files", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_get_file_metadata_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route("/files/{id}", actix_web::web::get().to(get_file_metadata)),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::get()
            .uri(&format!("/files/{}", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_download_file_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/files/{id}/download",
                    actix_web::web::get().to(download_file),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::get()
            .uri(&format!("/files/{}/download", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_restore_file_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/files/{id}/restore",
                    actix_web::web::post().to(restore_file),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/files/{}/restore", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_search_files_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{project_id}/files/search",
                    actix_web::web::post().to(search_files),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/projects/{}/files/search", id))
            .set_json(serde_json::json!({}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_bulk_delete_preview_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{project_id}/files/bulk-delete/preview",
                    actix_web::web::post().to(bulk_delete_preview),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/projects/{}/files/bulk-delete/preview", id))
            .set_json(serde_json::json!({"size_min": 1024}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_bulk_delete_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{project_id}/files/bulk-delete",
                    actix_web::web::post().to(bulk_delete),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/projects/{}/files/bulk-delete", id))
            .set_json(serde_json::json!({"size_min": 1024}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_search_summary_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{project_id}/files/search/summary",
                    actix_web::web::post().to(search_summary),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/projects/{}/files/search/summary", id))
            .set_json(serde_json::json!({}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }
}
