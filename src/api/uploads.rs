//! Resumable chunked uploads implementing a subset of the tus 1.0.0 protocol.
//!
//! Large files exceed the Cloudflare request-body limit when sent as one POST,
//! so the client (Uppy + @uppy/tus) splits them into sub-limit chunks. Each
//! chunk is appended to a temp file on the shared local volume; once the file is
//! complete it is handed to `FileService::finalize_upload`, which hashes and
//! stores it exactly like a normal upload (dedup, content-addressing, sync).
//!
//! Supported tus features: `creation`, `termination`. One upload = one session.

use actix_web::{http::header, web, HttpRequest, HttpResponse};
use base64::Engine;
use chrono::{Duration as ChronoDuration, Utc};
use futures::StreamExt;
use sqlx::PgPool;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::db::models::{CreateUploadSession, Project, UploadSession};
use crate::error::AppError;
use crate::services::FileService;

use super::auth::AuthenticatedUser;

const TUS_VERSION: &str = "1.0.0";

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Directory under the local temp path where in-progress chunk files live.
fn uploads_dir(config: &AppConfig) -> PathBuf {
    PathBuf::from(&config.storage.local_temp_path).join("uploads")
}

fn temp_path_for(config: &AppConfig, id: Uuid) -> PathBuf {
    uploads_dir(config).join(format!("{}.part", id))
}

/// Decode a tus `Upload-Metadata` header (`key b64val,key2 b64val2,...`) into pairs.
fn parse_tus_metadata(raw: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for pair in raw.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let mut it = pair.splitn(2, ' ');
        let key = it.next().unwrap_or("").to_string();
        let val_b64 = it.next().unwrap_or("");
        let val = base64::engine::general_purpose::STANDARD
            .decode(val_b64)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();
        if !key.is_empty() {
            out.insert(key, val);
        }
    }
    out
}

fn header_str(req: &HttpRequest, name: &str) -> Option<String> {
    req.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

async fn require_write(
    pool: &PgPool,
    user: &AuthenticatedUser,
    project_id: Uuid,
) -> Result<Project, AppError> {
    let project = Project::find_by_id(pool, project_id).await?;
    super::files::require_project_write_access(pool, user, &project).await?;
    Ok(project)
}

/// Load a session and verify the caller owns it (creator or admin).
async fn load_owned_session(
    pool: &PgPool,
    user: &AuthenticatedUser,
    id: Uuid,
) -> Result<UploadSession, AppError> {
    let session = UploadSession::find_by_id(pool, id).await?;
    if !user.is_admin() && session.user_id != user.user_id {
        return Err(AppError::Forbidden(
            "Access denied: not the upload owner".to_string(),
        ));
    }
    Ok(session)
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// OPTIONS — tus capability discovery.
async fn options(config: web::Data<AppConfig>) -> HttpResponse {
    let mut builder = HttpResponse::NoContent();
    builder
        .insert_header(("Tus-Resumable", TUS_VERSION))
        .insert_header(("Tus-Version", TUS_VERSION))
        .insert_header(("Tus-Extension", "creation,termination"));
    if config.upload.max_file_size > 0 {
        builder.insert_header(("Tus-Max-Size", config.upload.max_file_size.to_string()));
    }
    builder.finish()
}

/// POST /projects/{id}/uploads — tus creation. Allocates a session + temp file.
async fn create(
    user: AuthenticatedUser,
    pool: web::Data<PgPool>,
    config: web::Data<AppConfig>,
    path: web::Path<Uuid>,
    req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    require_write(pool.get_ref(), &user, project_id).await?;

    // Upload-Length: total size of the file to be uploaded.
    let total_size: i64 = header_str(&req, "Upload-Length")
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AppError::BadRequest("Missing or invalid Upload-Length".to_string()))?;
    if total_size <= 0 {
        return Err(AppError::BadRequest("Upload-Length must be positive".to_string()));
    }
    let max = config.upload.max_file_size;
    if max > 0 && total_size as u64 > max {
        return Err(AppError::BadRequest(format!(
            "File exceeds maximum allowed size of {} bytes",
            max
        )));
    }

    // Parse tus metadata for filename / filetype / our flat metadata JSON.
    let meta_map = header_str(&req, "Upload-Metadata")
        .map(|h| parse_tus_metadata(&h))
        .unwrap_or_default();
    let original_name = meta_map
        .get("filename")
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unnamed".to_string());
    let content_type = meta_map
        .get("filetype")
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let metadata = match meta_map.get("meta") {
        Some(json_str) if !json_str.is_empty() => {
            let value: serde_json::Value = serde_json::from_str(json_str)
                .map_err(|e| AppError::BadRequest(format!("Invalid metadata JSON: {}", e)))?;
            super::files::validate_flat_metadata(&value)?;
            value
        }
        _ => serde_json::json!({}),
    };

    let id = Uuid::new_v4();
    let temp_path = temp_path_for(&config, id);
    tokio::fs::create_dir_all(uploads_dir(&config))
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create uploads dir: {}", e)))?;
    // Create an empty temp file up front.
    tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create temp file: {}", e)))?;

    let expires_at = Utc::now() + ChronoDuration::seconds(config.upload.session_ttl_secs as i64);
    let session = UploadSession::create(
        pool.get_ref(),
        &CreateUploadSession {
            project_id,
            user_id: user.user_id,
            original_name,
            content_type,
            total_size,
            temp_path: temp_path.to_string_lossy().to_string(),
            metadata,
            expires_at,
        },
    )
    .await?;

    Ok(HttpResponse::Created()
        .insert_header(("Tus-Resumable", TUS_VERSION))
        .insert_header(("Location", format!("/api/uploads/{}", session.id)))
        .finish())
}

/// HEAD /uploads/{id} — report current offset for resumption.
async fn head(
    user: AuthenticatedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let session = load_owned_session(pool.get_ref(), &user, path.into_inner()).await?;
    Ok(HttpResponse::Ok()
        .insert_header(("Tus-Resumable", TUS_VERSION))
        .insert_header(("Upload-Offset", session.offset_bytes.to_string()))
        .insert_header(("Upload-Length", session.total_size.to_string()))
        .insert_header((header::CACHE_CONTROL, "no-store"))
        .finish())
}

/// PATCH /uploads/{id} — append a chunk at the given offset; finalize when complete.
async fn patch(
    user: AuthenticatedUser,
    pool: web::Data<PgPool>,
    config: web::Data<AppConfig>,
    file_service: web::Data<FileService>,
    path: web::Path<Uuid>,
    req: HttpRequest,
    mut payload: web::Payload,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let session = load_owned_session(pool.get_ref(), &user, id).await?;

    if session.status != "in_progress" {
        return Err(AppError::BadRequest(format!(
            "Upload session is {}",
            session.status
        )));
    }

    let client_offset: i64 = header_str(&req, "Upload-Offset")
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AppError::BadRequest("Missing or invalid Upload-Offset".to_string()))?;
    if client_offset != session.offset_bytes {
        // tus: offset conflict
        return Err(AppError::Conflict(format!(
            "Upload-Offset {} does not match server offset {}",
            client_offset, session.offset_bytes
        )));
    }

    let temp_path = PathBuf::from(&session.temp_path);
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(&temp_path)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to open temp file: {}", e)))?;

    let mut written: i64 = 0;
    while let Some(chunk) = payload.next().await {
        let chunk = chunk.map_err(|e| AppError::BadRequest(format!("Payload read error: {}", e)))?;
        // Guard against overrunning the declared total size.
        if session.offset_bytes + written + chunk.len() as i64 > session.total_size {
            return Err(AppError::BadRequest(
                "Chunk exceeds declared Upload-Length".to_string(),
            ));
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to write chunk: {}", e)))?;
        written += chunk.len() as i64;
    }
    file.flush()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to flush temp file: {}", e)))?;

    let new_offset = session.offset_bytes + written;
    let updated = UploadSession::advance_offset(pool.get_ref(), id, session.offset_bytes, new_offset)
        .await?
        .ok_or_else(|| {
            AppError::Conflict("Concurrent modification of upload session".to_string())
        })?;

    // Finalize once the whole file has arrived.
    if updated.offset_bytes >= updated.total_size {
        match file_service
            .finalize_upload(
                updated.project_id,
                &updated.original_name,
                &updated.content_type,
                &temp_path,
                updated.metadata.clone(),
            )
            .await
        {
            Ok(_) => {
                UploadSession::mark_status(pool.get_ref(), id, "completed").await?;
                let _ = tokio::fs::remove_file(&temp_path).await;
            }
            Err(e) => {
                // Keep the temp file so the client can retry completion.
                tracing::error!(upload_id = %id, error = %e, "Upload finalize failed");
                return Err(e);
            }
        }
    }

    let _ = &config; // retained for symmetry; temp path is resolved from the session
    Ok(HttpResponse::NoContent()
        .insert_header(("Tus-Resumable", TUS_VERSION))
        .insert_header(("Upload-Offset", updated.offset_bytes.to_string()))
        .finish())
}

/// DELETE /uploads/{id} — abort an upload and remove its temp file.
async fn terminate(
    user: AuthenticatedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let session = load_owned_session(pool.get_ref(), &user, id).await?;
    let _ = tokio::fs::remove_file(&session.temp_path).await;
    UploadSession::delete(pool.get_ref(), id).await?;
    Ok(HttpResponse::NoContent()
        .insert_header(("Tus-Resumable", TUS_VERSION))
        .finish())
}

// ─── Route configuration ─────────────────────────────────────────────────────

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/projects/{project_id}/uploads")
            .route(web::post().to(create))
            .route(web::method(actix_web::http::Method::OPTIONS).to(options)),
    )
    .service(
        web::resource("/uploads/{id}")
            .route(web::head().to(head))
            .route(web::patch().to(patch))
            .route(web::delete().to(terminate))
            .route(web::method(actix_web::http::Method::OPTIONS).to(options)),
    );
}
