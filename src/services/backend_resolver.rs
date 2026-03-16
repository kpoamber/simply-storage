use std::sync::Arc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::models::{Project, ProjectStorage, Storage};
use crate::error::AppResult;
use crate::storage::registry::create_backend;
use crate::storage::traits::StorageBackend;
use crate::storage::StorageRegistry;

/// Apply the effective container name to a storage config based on storage type.
/// For cloud storages (S3/GCS/Azure): sets bucket/container.
/// For filesystem storages (local/hetzner/ftp/sftp/samba): appends as subfolder to base path.
fn apply_container_to_config(
    config: &mut serde_json::Value,
    storage_type: &str,
    container: &str,
) {
    match storage_type {
        "s3" | "gcs" => {
            config["bucket"] = serde_json::Value::String(container.to_string());
        }
        "azure" => {
            config["container"] = serde_json::Value::String(container.to_string());
        }
        "local" => {
            // Append container as subfolder under the base path
            let base = config["path"].as_str().unwrap_or("/data");
            let new_path = format!("{}/{}", base.trim_end_matches('/'), container);
            config["path"] = serde_json::Value::String(new_path);
        }
        "hetzner" | "ftp" | "sftp" | "samba" => {
            let base = config["base_path"].as_str().unwrap_or("");
            let new_path = if base.is_empty() {
                container.to_string()
            } else {
                format!("{}/{}", base.trim_end_matches('/'), container)
            };
            config["base_path"] = serde_json::Value::String(new_path);
        }
        _ => {}
    }
}

/// Resolve the effective container name for a project-storage pair.
/// Priority: container_override > project slug + short suffix.
/// The suffix is derived from the assignment ID to ensure uniqueness across storages
/// and avoid collisions with pre-existing containers.
fn effective_container(project: &Project, assignment: Option<&ProjectStorage>) -> String {
    if let Some(ps) = assignment {
        if let Some(ref override_name) = ps.container_override {
            if !override_name.is_empty() {
                return override_name.clone();
            }
        }
        // Use first 6 hex chars of the assignment UUID as a stable suffix
        let suffix = &ps.id.to_string()[..6];
        return format!("{}-{}", project.slug, suffix);
    }
    // No assignment — fallback to slug only (shouldn't happen in normal flow)
    project.slug.clone()
}

/// Resolve a storage backend for a specific project, applying container/prefix overrides.
///
/// The container is derived from:
/// 1. `project_storages.container_override` if set
/// 2. `project.slug` as default
///
/// For cloud storages, auto-creates the bucket/container if it doesn't exist.
pub async fn resolve_project_backend(
    pool: &PgPool,
    _registry: &StorageRegistry,
    storage: &Storage,
    project_id: Uuid,
    hmac_secret: &str,
) -> AppResult<Arc<dyn StorageBackend>> {
    let project = Project::find_by_id(pool, project_id).await?;
    let assignment =
        ProjectStorage::find_for_project_and_storage(pool, project_id, storage.id).await?;

    let container = effective_container(&project, assignment.as_ref());

    let mut config = storage.config.clone();
    apply_container_to_config(&mut config, &storage.storage_type, &container);

    if let Some(ref ps) = assignment {
        if let Some(ref prefix) = ps.prefix_override {
            config["prefix"] = serde_json::Value::String(prefix.clone());
        }
    }

    let backend = create_backend(&storage.storage_type, &config, hmac_secret).await?;

    // Auto-create container/bucket if supported
    if backend.supports_containers() {
        if let Err(e) = backend.create_container(&container).await {
            // Ignore "already exists" errors — only log unexpected failures
            tracing::debug!(
                container = %container,
                storage_type = %storage.storage_type,
                error = %e,
                "Container creation skipped (may already exist)"
            );
        }
    }

    Ok(backend)
}

/// Resolve all possible backends for a file location across multiple project references.
/// Returns override-based backends first, then the default registry backend as fallback.
pub async fn resolve_backends_for_location(
    pool: &PgPool,
    registry: &StorageRegistry,
    storage_id: &Uuid,
    file_refs: &[crate::db::models::FileReference],
    hmac_secret: &str,
) -> Vec<Arc<dyn StorageBackend>> {
    let mut backends: Vec<Arc<dyn StorageBackend>> = Vec::new();

    if let Ok(storage) = Storage::find_by_id(pool, *storage_id).await {
        for fref in file_refs {
            if let Ok(backend) =
                resolve_project_backend(pool, registry, &storage, fref.project_id, hmac_secret)
                    .await
            {
                backends.push(backend);
            }
        }
    }

    // Fallback: default backend from registry only if no project backends were resolved.
    // The registry backend uses the storage-level config (no project container),
    // so it should only be used when there's no project context.
    if backends.is_empty() {
        if let Ok(default_backend) = registry.get(storage_id).await {
            backends.push(default_backend);
        }
    }

    backends
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_container_s3() {
        let mut config = serde_json::json!({"bucket": "old-bucket", "region": "us-east-1"});
        apply_container_to_config(&mut config, "s3", "my-project");
        assert_eq!(config["bucket"], "my-project");
    }

    #[test]
    fn test_apply_container_gcs() {
        let mut config = serde_json::json!({"bucket": "old-bucket"});
        apply_container_to_config(&mut config, "gcs", "my-project");
        assert_eq!(config["bucket"], "my-project");
    }

    #[test]
    fn test_apply_container_azure() {
        let mut config = serde_json::json!({"container": "old-container"});
        apply_container_to_config(&mut config, "azure", "my-project");
        assert_eq!(config["container"], "my-project");
    }

    #[test]
    fn test_apply_container_local() {
        let mut config = serde_json::json!({"path": "/data/storage"});
        apply_container_to_config(&mut config, "local", "my-project");
        assert_eq!(config["path"], "/data/storage/my-project");
    }

    #[test]
    fn test_apply_container_hetzner() {
        let mut config = serde_json::json!({"base_path": "/files"});
        apply_container_to_config(&mut config, "hetzner", "my-project");
        assert_eq!(config["base_path"], "/files/my-project");
    }

    #[test]
    fn test_apply_container_ftp_empty_base() {
        let mut config = serde_json::json!({"base_path": ""});
        apply_container_to_config(&mut config, "ftp", "my-project");
        assert_eq!(config["base_path"], "my-project");
    }

    #[test]
    fn test_effective_container_with_override() {
        let project = Project {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test-project".to_string(),
            hot_to_cold_days: None,
            owner_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        };
        let ps = ProjectStorage {
            id: Uuid::new_v4(),
            project_id: project.id,
            storage_id: Uuid::new_v4(),
            container_override: Some("custom-bucket".to_string()),
            prefix_override: None,
            is_active: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        assert_eq!(effective_container(&project, Some(&ps)), "custom-bucket");
    }

    #[test]
    fn test_effective_container_defaults_to_slug_with_suffix() {
        let project = Project {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test-project".to_string(),
            hot_to_cold_days: None,
            owner_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        };
        let ps = ProjectStorage {
            id: Uuid::new_v4(),
            project_id: project.id,
            storage_id: Uuid::new_v4(),
            container_override: None,
            prefix_override: None,
            is_active: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let result = effective_container(&project, Some(&ps));
        let suffix = &ps.id.to_string()[..6];
        assert_eq!(result, format!("test-project-{}", suffix));
        assert!(result.starts_with("test-project-"));
        assert_eq!(result.len(), "test-project-".len() + 6);
    }

    #[test]
    fn test_effective_container_no_assignment_fallback() {
        let project = Project {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test-project".to_string(),
            hot_to_cold_days: None,
            owner_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        };
        assert_eq!(effective_container(&project, None), "test-project");
    }

    #[test]
    fn test_effective_container_empty_override_uses_slug_with_suffix() {
        let project = Project {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test-project".to_string(),
            hot_to_cold_days: None,
            owner_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        };
        let ps = ProjectStorage {
            id: Uuid::new_v4(),
            project_id: project.id,
            storage_id: Uuid::new_v4(),
            container_override: Some("".to_string()),
            prefix_override: None,
            is_active: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let result = effective_container(&project, Some(&ps));
        assert!(result.starts_with("test-project-"));
    }

    #[test]
    fn test_effective_container_suffix_is_stable() {
        let project = Project {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "my-proj".to_string(),
            hot_to_cold_days: None,
            owner_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        };
        let ps = ProjectStorage {
            id: Uuid::new_v4(),
            project_id: project.id,
            storage_id: Uuid::new_v4(),
            container_override: None,
            prefix_override: None,
            is_active: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        // Same input → same output
        assert_eq!(
            effective_container(&project, Some(&ps)),
            effective_container(&project, Some(&ps))
        );
    }
}
