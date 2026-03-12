use actix_web::{web, HttpResponse};
use serde::Serialize;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use super::PaginationParams;
use crate::config::AppConfig;
use crate::db::models::{CreateStorage, FileLocation, Storage, UpdateStorage};
use crate::error::AppError;
use crate::storage::registry::{create_backend, StorageRegistry};

// ─── Response types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct StorageWithStats {
    #[serde(flatten)]
    pub storage: Storage,
    pub file_count: i64,
    pub used_space: i64,
}

// ─── Handlers ───────────────────────────────────────────────────────────────────

async fn create_storage(
    pool: web::Data<PgPool>,
    registry: web::Data<Arc<StorageRegistry>>,
    config: web::Data<AppConfig>,
    body: web::Json<CreateStorage>,
) -> Result<HttpResponse, AppError> {
    let storage = Storage::create(pool.get_ref(), &body).await?;

    // Register the backend in the registry so it's immediately usable
    if storage.enabled {
        match create_backend(&storage.storage_type, &storage.config, &config.storage.hmac_secret).await {
            Ok(backend) => {
                registry.register(storage.id, backend).await;
                tracing::info!(storage_id = %storage.id, "Registered new storage backend");
            }
            Err(e) => {
                tracing::warn!(storage_id = %storage.id, error = %e, "Created storage but failed to initialize backend");
            }
        }
    }

    Ok(HttpResponse::Created().json(storage))
}

async fn list_storages(pool: web::Data<PgPool>) -> Result<HttpResponse, AppError> {
    let storages = Storage::list(pool.get_ref()).await?;

    let mut result = Vec::with_capacity(storages.len());
    for storage in storages {
        let stats_row = sqlx::query(
            r#"SELECT COUNT(*)::bigint, COALESCE(SUM(f.size), 0)::bigint
               FROM file_locations fl
               JOIN files f ON f.id = fl.file_id
               WHERE fl.storage_id = $1 AND fl.status = 'synced'"#,
        )
        .bind(storage.id)
        .fetch_one(pool.get_ref())
        .await?;

        let file_count: i64 = sqlx::Row::get(&stats_row, 0);
        let used_space: i64 = sqlx::Row::get(&stats_row, 1);

        result.push(StorageWithStats {
            storage: storage.redacted(),
            file_count,
            used_space,
        });
    }

    Ok(HttpResponse::Ok().json(result))
}

async fn get_storage(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let storage_id = path.into_inner();
    let storage = Storage::find_by_id(pool.get_ref(), storage_id).await?;

    let stats_row = sqlx::query(
        r#"SELECT COUNT(*)::bigint, COALESCE(SUM(f.size), 0)::bigint
           FROM file_locations fl
           JOIN files f ON f.id = fl.file_id
           WHERE fl.storage_id = $1 AND fl.status = 'synced'"#,
    )
    .bind(storage_id)
    .fetch_one(pool.get_ref())
    .await?;

    let file_count: i64 = sqlx::Row::get(&stats_row, 0);
    let used_space: i64 = sqlx::Row::get(&stats_row, 1);

    Ok(HttpResponse::Ok().json(StorageWithStats {
        storage: storage.redacted(),
        file_count,
        used_space,
    }))
}

async fn update_storage(
    pool: web::Data<PgPool>,
    registry: web::Data<Arc<StorageRegistry>>,
    config: web::Data<AppConfig>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateStorage>,
) -> Result<HttpResponse, AppError> {
    let storage_id = path.into_inner();
    let storage = Storage::update(pool.get_ref(), storage_id, &body).await?;

    // Re-create and re-register the backend if config or type changed
    if storage.enabled {
        match create_backend(&storage.storage_type, &storage.config, &config.storage.hmac_secret).await {
            Ok(backend) => {
                registry.register(storage.id, backend).await;
                tracing::info!(storage_id = %storage.id, "Re-registered updated storage backend");
            }
            Err(e) => {
                tracing::warn!(storage_id = %storage.id, error = %e, "Updated storage but failed to re-initialize backend");
            }
        }
    } else {
        registry.unregister(&storage.id).await;
    }

    Ok(HttpResponse::Ok().json(storage))
}

async fn disable_storage(
    pool: web::Data<PgPool>,
    registry: web::Data<Arc<StorageRegistry>>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let storage_id = path.into_inner();
    let storage = Storage::update_enabled(pool.get_ref(), storage_id, false).await?;
    registry.unregister(&storage_id).await;
    Ok(HttpResponse::Ok().json(storage))
}

async fn list_storage_files(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    query: web::Query<PaginationParams>,
) -> Result<HttpResponse, AppError> {
    let storage_id = path.into_inner();
    // Verify storage exists
    let _storage = Storage::find_by_id(pool.get_ref(), storage_id).await?;

    let locations = sqlx::query_as::<_, FileLocation>(
        r#"SELECT * FROM file_locations
           WHERE storage_id = $1
           ORDER BY created_at DESC
           LIMIT $2 OFFSET $3"#,
    )
    .bind(storage_id)
    .bind(query.limit())
    .bind(query.offset())
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(locations))
}

// ─── Route configuration ────────────────────────────────────────────────────────

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/storages")
            .route(web::post().to(create_storage))
            .route(web::get().to(list_storages)),
    )
    .service(
        web::resource("/storages/{id}")
            .route(web::get().to(get_storage))
            .route(web::put().to(update_storage))
            .route(web::delete().to(disable_storage)),
    )
    .service(
        web::resource("/storages/{id}/files").route(web::get().to(list_storage_files)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::CreateStorage;

    #[test]
    fn test_create_storage_deserialization() {
        let json = serde_json::json!({
            "name": "S3 Primary",
            "storage_type": "s3",
            "config": {"bucket": "my-bucket", "region": "us-east-1"},
            "is_hot": true
        });
        let input: CreateStorage = serde_json::from_value(json).unwrap();
        assert_eq!(input.name, "S3 Primary");
        assert_eq!(input.storage_type, "s3");
        assert_eq!(input.config["bucket"], "my-bucket");
        assert_eq!(input.is_hot, Some(true));
    }

    #[test]
    fn test_create_storage_minimal() {
        let json = serde_json::json!({
            "name": "Local",
            "storage_type": "local",
            "config": {"path": "/data"}
        });
        let input: CreateStorage = serde_json::from_value(json).unwrap();
        assert!(input.is_hot.is_none());
        assert!(input.project_id.is_none());
        assert!(input.enabled.is_none());
    }

    #[test]
    fn test_update_storage_deserialization() {
        let json = serde_json::json!({
            "name": "Updated Name",
            "enabled": false
        });
        let input: UpdateStorage = serde_json::from_value(json).unwrap();
        assert_eq!(input.name, Some("Updated Name".to_string()));
        assert_eq!(input.enabled, Some(false));
        assert!(input.storage_type.is_none());
        assert!(input.config.is_none());
    }

    #[test]
    fn test_update_storage_empty() {
        let json = serde_json::json!({});
        let input: UpdateStorage = serde_json::from_value(json).unwrap();
        assert!(input.name.is_none());
        assert!(input.storage_type.is_none());
        assert!(input.config.is_none());
        assert!(input.is_hot.is_none());
        assert!(input.project_id.is_none());
        assert!(input.enabled.is_none());
    }

    #[test]
    fn test_storage_with_stats_serialization() {
        let now = chrono::Utc::now();
        let storage = Storage {
            id: uuid::Uuid::new_v4(),
            name: "Test Storage".to_string(),
            storage_type: "local".to_string(),
            config: serde_json::json!({"path": "/data"}),
            is_hot: true,
            project_id: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        };
        let with_stats = StorageWithStats {
            storage,
            file_count: 42,
            used_space: 1_048_576,
        };
        let json = serde_json::to_value(&with_stats).unwrap();
        assert_eq!(json["name"], "Test Storage");
        assert_eq!(json["file_count"], 42);
        assert_eq!(json["used_space"], 1_048_576);
        assert!(json["is_hot"].as_bool().unwrap());
    }

    #[test]
    fn test_create_storage_missing_required() {
        let json = serde_json::json!({
            "name": "Incomplete"
        });
        let result: Result<CreateStorage, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }
}
