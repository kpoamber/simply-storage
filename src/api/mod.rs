pub mod bulk;
pub mod files;
pub mod projects;
pub mod storages;

use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::db::models::{Node, Project, Storage, SyncTask};
use crate::error::AppError;

// ─── Common types ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

impl PaginationParams {
    pub fn limit(&self) -> i64 {
        self.per_page.unwrap_or(50).clamp(1, 100)
    }

    pub fn offset(&self) -> i64 {
        self.page
            .unwrap_or(1)
            .max(1)
            .saturating_sub(1)
            .saturating_mul(self.limit())
    }
}

// ─── System stats ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SystemStats {
    pub total_files: i64,
    pub total_storage_used: i64,
    pub pending_sync_tasks: i64,
}

async fn system_stats(pool: web::Data<PgPool>) -> Result<HttpResponse, AppError> {
    let file_row = sqlx::query("SELECT COUNT(*)::bigint, COALESCE(SUM(size), 0)::bigint FROM files")
        .fetch_one(pool.get_ref())
        .await?;
    let total_files: i64 = file_row.get(0);
    let total_storage_used: i64 = file_row.get(1);

    let pending_row =
        sqlx::query("SELECT COUNT(*)::bigint FROM sync_tasks WHERE status = 'pending'")
            .fetch_one(pool.get_ref())
            .await?;
    let pending_sync_tasks: i64 = pending_row.get(0);

    Ok(HttpResponse::Ok().json(SystemStats {
        total_files,
        total_storage_used,
        pending_sync_tasks,
    }))
}

// ─── Sync tasks ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SyncTaskFilter {
    pub status: Option<String>,
    pub storage_id: Option<Uuid>,
}

async fn list_sync_tasks(
    pool: web::Data<PgPool>,
    query: web::Query<SyncTaskFilter>,
) -> Result<HttpResponse, AppError> {
    let tasks = SyncTask::list_filtered(
        pool.get_ref(),
        query.status.as_deref(),
        query.storage_id,
    )
    .await?;
    Ok(HttpResponse::Ok().json(tasks))
}

// ─── Config export ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ConfigExport {
    pub config: AppConfig,
    pub projects: Vec<Project>,
    pub storages: Vec<Storage>,
}

async fn config_export(
    pool: web::Data<PgPool>,
    config: web::Data<AppConfig>,
) -> Result<HttpResponse, AppError> {
    let projects = Project::list(pool.get_ref()).await?;
    let storages = Storage::list(pool.get_ref()).await?;

    // Redact sensitive values from config
    let mut safe_config = config.get_ref().clone();
    safe_config.storage.hmac_secret = "***".to_string();
    safe_config.database.url = "***".to_string();

    Ok(HttpResponse::Ok().json(ConfigExport {
        config: safe_config,
        projects,
        storages: storages.into_iter().map(|s| s.redacted()).collect(),
    }))
}

// ─── Nodes ──────────────────────────────────────────────────────────────────────

async fn list_nodes(pool: web::Data<PgPool>) -> Result<HttpResponse, AppError> {
    // Consider nodes active if heartbeat within the last 90 seconds (3x the 30s interval)
    let nodes = Node::list_active(pool.get_ref(), 90).await?;
    Ok(HttpResponse::Ok().json(nodes))
}

// ─── Route configuration ────────────────────────────────────────────────────────

pub fn configure_api_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .configure(projects::configure)
            .configure(files::configure)
            .configure(storages::configure)
            .configure(bulk::configure)
            .route("/system/stats", web::get().to(system_stats))
            .route("/sync-tasks", web::get().to(list_sync_tasks))
            .route("/system/config-export", web::get().to(config_export))
            .route("/system/nodes", web::get().to(list_nodes)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagination_defaults() {
        let params = PaginationParams {
            page: None,
            per_page: None,
        };
        assert_eq!(params.limit(), 50);
        assert_eq!(params.offset(), 0);
    }

    #[test]
    fn test_pagination_custom() {
        let params = PaginationParams {
            page: Some(3),
            per_page: Some(20),
        };
        assert_eq!(params.limit(), 20);
        assert_eq!(params.offset(), 40);
    }

    #[test]
    fn test_pagination_clamping() {
        let params = PaginationParams {
            page: Some(0),
            per_page: Some(200),
        };
        assert_eq!(params.limit(), 100);
        assert_eq!(params.offset(), 0);

        let params2 = PaginationParams {
            page: Some(-1),
            per_page: Some(0),
        };
        assert_eq!(params2.limit(), 1);
        assert_eq!(params2.offset(), 0);
    }

    #[test]
    fn test_system_stats_serialization() {
        let stats = SystemStats {
            total_files: 42,
            total_storage_used: 1_073_741_824,
            pending_sync_tasks: 5,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["total_files"], 42);
        assert_eq!(json["total_storage_used"], 1_073_741_824i64);
        assert_eq!(json["pending_sync_tasks"], 5);
    }

    #[test]
    fn test_sync_task_filter_deserialization() {
        let json = serde_json::json!({
            "status": "pending",
            "storage_id": "550e8400-e29b-41d4-a716-446655440000"
        });
        let filter: SyncTaskFilter = serde_json::from_value(json).unwrap();
        assert_eq!(filter.status.as_deref(), Some("pending"));
        assert!(filter.storage_id.is_some());
    }

    #[test]
    fn test_sync_task_filter_empty() {
        let json = serde_json::json!({});
        let filter: SyncTaskFilter = serde_json::from_value(json).unwrap();
        assert!(filter.status.is_none());
        assert!(filter.storage_id.is_none());
    }

    #[test]
    fn test_config_export_serialization() {
        let config = AppConfig::load_from("__nonexistent__").unwrap();
        let export = ConfigExport {
            config,
            projects: vec![],
            storages: vec![],
        };
        let json = serde_json::to_value(&export).unwrap();
        assert!(json["config"]["server"]["port"].is_number());
        assert!(json["projects"].is_array());
        assert!(json["storages"].is_array());
    }

    #[test]
    fn test_pagination_page_one() {
        let params = PaginationParams {
            page: Some(1),
            per_page: Some(10),
        };
        assert_eq!(params.offset(), 0);
        assert_eq!(params.limit(), 10);
    }

    #[test]
    fn test_pagination_page_two() {
        let params = PaginationParams {
            page: Some(2),
            per_page: Some(10),
        };
        assert_eq!(params.offset(), 10);
    }

    #[test]
    fn test_config_export_contains_required_fields() {
        let config = AppConfig::load_from("__nonexistent__").unwrap();
        let export = ConfigExport {
            config,
            projects: vec![],
            storages: vec![],
        };
        let json = serde_json::to_value(&export).unwrap();

        // Verify config-export returns valid config with all required sections
        assert!(json["config"]["server"]["host"].is_string());
        assert!(json["config"]["server"]["port"].is_number());
        assert!(json["config"]["database"]["url"].is_string());
        assert!(json["config"]["database"]["max_connections"].is_number());
        assert!(json["config"]["storage"]["hmac_secret"].is_string());
        assert!(json["config"]["storage"]["local_temp_path"].is_string());
        assert!(json["config"]["sync"]["num_workers"].is_number());
        assert!(json["config"]["sync"]["max_retries"].is_number());
    }

    #[test]
    fn test_config_export_with_projects_and_storages() {
        let config = AppConfig::load_from("__nonexistent__").unwrap();
        let now = chrono::Utc::now();
        let project = Project {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test".to_string(),
            hot_to_cold_days: Some(7),
            created_at: now,
            updated_at: now,
        };
        let storage = Storage {
            id: Uuid::new_v4(),
            name: "Local".to_string(),
            storage_type: "local".to_string(),
            config: serde_json::json!({"path": "/data"}),
            is_hot: true,
            project_id: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        };
        let export = ConfigExport {
            config,
            projects: vec![project],
            storages: vec![storage],
        };
        let json = serde_json::to_value(&export).unwrap();

        assert_eq!(json["projects"].as_array().unwrap().len(), 1);
        assert_eq!(json["storages"].as_array().unwrap().len(), 1);
        assert_eq!(json["projects"][0]["name"], "Test");
        assert_eq!(json["storages"][0]["storage_type"], "local");
    }
}
