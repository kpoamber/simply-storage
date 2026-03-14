use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

// ─── Project ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub hot_to_cold_days: Option<i32>,
    pub owner_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Project with assignment role, returned when listing user's project assignments.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProjectAssignment {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub hot_to_cold_days: Option<i32>,
    pub owner_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub assignment_role: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateProject {
    pub name: String,
    pub slug: String,
    pub hot_to_cold_days: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub hot_to_cold_days: Option<Option<i32>>,
}

impl Project {
    pub async fn create(
        pool: &PgPool,
        input: &CreateProject,
        owner_id: Option<Uuid>,
    ) -> AppResult<Project> {
        let row = sqlx::query_as::<_, Project>(
            r#"INSERT INTO projects (name, slug, hot_to_cold_days, owner_id)
               VALUES ($1, $2, $3, $4)
               RETURNING *"#,
        )
        .bind(&input.name)
        .bind(&input.slug)
        .bind(input.hot_to_cold_days)
        .bind(owner_id)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> AppResult<Project> {
        sqlx::query_as::<_, Project>(
            "SELECT * FROM projects WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Project {} not found", id)))
    }

    pub async fn find_by_slug(pool: &PgPool, slug: &str) -> AppResult<Project> {
        sqlx::query_as::<_, Project>(
            "SELECT * FROM projects WHERE slug = $1 AND deleted_at IS NULL",
        )
        .bind(slug)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Project with slug '{}' not found", slug)))
    }

    pub async fn list(pool: &PgPool) -> AppResult<Vec<Project>> {
        let rows = sqlx::query_as::<_, Project>(
            "SELECT * FROM projects WHERE deleted_at IS NULL ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn list_for_owner(pool: &PgPool, owner_id: Uuid) -> AppResult<Vec<Project>> {
        let rows = sqlx::query_as::<_, Project>(
            "SELECT * FROM projects WHERE deleted_at IS NULL AND owner_id = $1 ORDER BY created_at DESC",
        )
        .bind(owner_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// List projects the user owns OR is a member of (via user_projects).
    pub async fn list_accessible(pool: &PgPool, user_id: Uuid) -> AppResult<Vec<Project>> {
        let rows = sqlx::query_as::<_, Project>(
            r#"SELECT * FROM projects p
               WHERE p.deleted_at IS NULL
               AND (p.owner_id = $1 OR EXISTS (
                   SELECT 1 FROM user_projects up WHERE up.project_id = p.id AND up.user_id = $1
               ))
               ORDER BY p.created_at DESC"#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn update(pool: &PgPool, id: Uuid, input: &UpdateProject) -> AppResult<Project> {
        let current = Self::find_by_id(pool, id).await?;

        let name = input.name.as_deref().unwrap_or(&current.name);
        let slug = input.slug.as_deref().unwrap_or(&current.slug);
        let hot_to_cold_days = match &input.hot_to_cold_days {
            Some(val) => *val,
            None => current.hot_to_cold_days,
        };

        let row = sqlx::query_as::<_, Project>(
            r#"UPDATE projects SET name = $1, slug = $2, hot_to_cold_days = $3
               WHERE id = $4
               RETURNING *"#,
        )
        .bind(name)
        .bind(slug)
        .bind(hot_to_cold_days)
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE projects SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Project {} not found", id)));
        }
        Ok(())
    }
}

// ─── Storage ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Storage {
    pub id: Uuid,
    pub name: String,
    pub storage_type: String,
    pub config: serde_json::Value,
    pub is_hot: bool,
    pub project_id: Option<Uuid>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const SENSITIVE_CONFIG_KEYS: &[&str] = &[
    "secret_access_key", "account_key", "password", "private_key_pem",
];

impl Storage {
    /// Return a copy with sensitive credential values redacted from the config JSON.
    pub fn redacted(&self) -> Storage {
        let mut s = self.clone();
        if let serde_json::Value::Object(ref mut map) = s.config {
            for key in SENSITIVE_CONFIG_KEYS {
                if map.contains_key(*key) {
                    map.insert((*key).to_string(), serde_json::Value::String("***".to_string()));
                }
            }
        }
        s
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateStorage {
    pub name: String,
    pub storage_type: String,
    pub config: serde_json::Value,
    pub is_hot: Option<bool>,
    pub project_id: Option<Uuid>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateStorage {
    pub name: Option<String>,
    pub storage_type: Option<String>,
    pub config: Option<serde_json::Value>,
    pub is_hot: Option<bool>,
    pub project_id: Option<Option<Uuid>>,
    pub enabled: Option<bool>,
}

impl Storage {
    pub async fn create(pool: &PgPool, input: &CreateStorage) -> AppResult<Storage> {
        let row = sqlx::query_as::<_, Storage>(
            r#"INSERT INTO storages (name, storage_type, config, is_hot, project_id, enabled)
               VALUES ($1, $2, $3, $4, $5, $6)
               RETURNING *"#,
        )
        .bind(&input.name)
        .bind(&input.storage_type)
        .bind(&input.config)
        .bind(input.is_hot.unwrap_or(true))
        .bind(input.project_id)
        .bind(input.enabled.unwrap_or(true))
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> AppResult<Storage> {
        sqlx::query_as::<_, Storage>("SELECT * FROM storages WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Storage {} not found", id)))
    }

    pub async fn list(pool: &PgPool) -> AppResult<Vec<Storage>> {
        let rows = sqlx::query_as::<_, Storage>(
            "SELECT * FROM storages ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn list_enabled(pool: &PgPool) -> AppResult<Vec<Storage>> {
        let rows = sqlx::query_as::<_, Storage>(
            "SELECT * FROM storages WHERE enabled = TRUE ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn list_for_project(pool: &PgPool, project_id: Uuid) -> AppResult<Vec<Storage>> {
        let rows = sqlx::query_as::<_, Storage>(
            r#"SELECT s.* FROM storages s
               JOIN project_storages ps ON ps.storage_id = s.id
               WHERE ps.project_id = $1 AND ps.is_active = TRUE AND s.enabled = TRUE
               ORDER BY s.is_hot DESC, s.created_at DESC"#,
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn update_enabled(pool: &PgPool, id: Uuid, enabled: bool) -> AppResult<Storage> {
        let row = sqlx::query_as::<_, Storage>(
            "UPDATE storages SET enabled = $1 WHERE id = $2 RETURNING *",
        )
        .bind(enabled)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Storage {} not found", id)))?;

        Ok(row)
    }

    pub async fn update(pool: &PgPool, id: Uuid, input: &UpdateStorage) -> AppResult<Storage> {
        let current = Self::find_by_id(pool, id).await?;

        let name = input.name.as_deref().unwrap_or(&current.name);
        let storage_type = input.storage_type.as_deref().unwrap_or(&current.storage_type);
        let config = input.config.as_ref().unwrap_or(&current.config);
        let is_hot = input.is_hot.unwrap_or(current.is_hot);
        let project_id = match &input.project_id {
            Some(val) => *val,
            None => current.project_id,
        };
        let enabled = input.enabled.unwrap_or(current.enabled);

        let row = sqlx::query_as::<_, Storage>(
            r#"UPDATE storages
               SET name = $1, storage_type = $2, config = $3, is_hot = $4, project_id = $5, enabled = $6
               WHERE id = $7
               RETURNING *"#,
        )
        .bind(name)
        .bind(storage_type)
        .bind(config)
        .bind(is_hot)
        .bind(project_id)
        .bind(enabled)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Storage {} not found", id)))?;

        Ok(row)
    }
}

// ─── File ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct File {
    pub id: Uuid,
    pub hash_sha256: String,
    pub size: i64,
    pub content_type: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFile {
    pub hash_sha256: String,
    pub size: i64,
    pub content_type: String,
}

impl File {
    pub async fn create(pool: &PgPool, input: &CreateFile) -> AppResult<File> {
        let row = sqlx::query_as::<_, File>(
            r#"INSERT INTO files (hash_sha256, size, content_type)
               VALUES ($1, $2, $3)
               RETURNING *"#,
        )
        .bind(&input.hash_sha256)
        .bind(input.size)
        .bind(&input.content_type)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> AppResult<File> {
        sqlx::query_as::<_, File>("SELECT * FROM files WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("File {} not found", id)))
    }

    pub async fn find_by_hash(pool: &PgPool, hash: &str) -> AppResult<Option<File>> {
        let row = sqlx::query_as::<_, File>(
            "SELECT * FROM files WHERE hash_sha256 = $1",
        )
        .bind(hash)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    pub async fn create_or_find(pool: &PgPool, input: &CreateFile) -> AppResult<(File, bool)> {
        if let Some(existing) = Self::find_by_hash(pool, &input.hash_sha256).await? {
            return Ok((existing, false));
        }

        match Self::create(pool, input).await {
            Ok(file) => Ok((file, true)),
            Err(AppError::Database(ref e)) if is_unique_violation(e) => {
                let file = Self::find_by_hash(pool, &input.hash_sha256)
                    .await?
                    .ok_or_else(|| {
                        AppError::Internal("File disappeared after conflict".to_string())
                    })?;
                Ok((file, false))
            }
            Err(e) => Err(e),
        }
    }
}

// ─── FileReference ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FileReference {
    pub id: Uuid,
    pub file_id: Uuid,
    pub project_id: Uuid,
    pub original_name: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFileReference {
    pub file_id: Uuid,
    pub project_id: Uuid,
    pub original_name: String,
    #[serde(default = "default_empty_object")]
    pub metadata: serde_json::Value,
}

fn default_empty_object() -> serde_json::Value {
    serde_json::json!({})
}

impl FileReference {
    pub async fn create(pool: &PgPool, input: &CreateFileReference) -> AppResult<FileReference> {
        let row = sqlx::query_as::<_, FileReference>(
            r#"INSERT INTO file_references (file_id, project_id, original_name, metadata)
               VALUES ($1, $2, $3, $4)
               RETURNING *"#,
        )
        .bind(input.file_id)
        .bind(input.project_id)
        .bind(&input.original_name)
        .bind(&input.metadata)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn create_or_find(
        pool: &PgPool,
        input: &CreateFileReference,
    ) -> AppResult<FileReference> {
        match Self::create(pool, input).await {
            Ok(row) => Ok(row),
            Err(AppError::Database(ref e)) if is_unique_violation(e) => {
                let row = sqlx::query_as::<_, FileReference>(
                    r#"SELECT * FROM file_references
                       WHERE file_id = $1 AND project_id = $2 AND original_name = $3"#,
                )
                .bind(input.file_id)
                .bind(input.project_id)
                .bind(&input.original_name)
                .fetch_one(pool)
                .await?;
                Ok(row)
            }
            Err(e) => Err(e),
        }
    }

    pub async fn list_for_project(
        pool: &PgPool,
        project_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> AppResult<Vec<FileReference>> {
        let rows = sqlx::query_as::<_, FileReference>(
            r#"SELECT * FROM file_references
               WHERE project_id = $1
               ORDER BY created_at DESC
               LIMIT $2 OFFSET $3"#,
        )
        .bind(project_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM file_references WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "File reference {} not found",
                id
            )));
        }
        Ok(())
    }

    pub async fn find_by_file_id(pool: &PgPool, file_id: Uuid) -> AppResult<Vec<FileReference>> {
        let rows = sqlx::query_as::<_, FileReference>(
            "SELECT * FROM file_references WHERE file_id = $1",
        )
        .bind(file_id)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn delete_by_file_and_project(
        pool: &PgPool,
        file_id: Uuid,
        project_id: Uuid,
    ) -> AppResult<()> {
        let result = sqlx::query(
            "DELETE FROM file_references WHERE file_id = $1 AND project_id = $2",
        )
        .bind(file_id)
        .bind(project_id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                "File reference not found for this file and project".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn update_metadata(
        pool: &PgPool,
        id: Uuid,
        metadata: &serde_json::Value,
    ) -> AppResult<FileReference> {
        let row = sqlx::query_as::<_, FileReference>(
            r#"UPDATE file_references SET metadata = $1
               WHERE id = $2
               RETURNING *"#,
        )
        .bind(metadata)
        .bind(id)
        .fetch_optional(pool)
        .await?;

        row.ok_or_else(|| AppError::NotFound(format!("File reference {} not found", id)))
    }
}

// ─── Metadata Search ──────────────────────────────────────────────────────────

/// Filter node for metadata search DSL.
/// Supports leaf matches, AND/OR/NOT logical composition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MetadataFilter {
    And { and: Vec<MetadataFilter> },
    Or { or: Vec<MetadataFilter> },
    Not { not: Box<MetadataFilter> },
    Leaf { key: String, value: serde_json::Value },
}

/// Compiles MetadataFilter tree into a SQL WHERE clause fragment with numbered parameters.
pub struct MetadataFilterCompiler {
    next_param: usize,
    params: Vec<serde_json::Value>,
}

impl MetadataFilterCompiler {
    /// Create a new compiler. `start_param` is the next available parameter index (1-based).
    pub fn new(start_param: usize) -> Self {
        Self {
            next_param: start_param,
            params: Vec::new(),
        }
    }

    /// Maximum allowed nesting depth for filter trees (prevents stack overflow from malicious input).
    const MAX_DEPTH: usize = 32;

    /// Compile a filter tree into a SQL fragment and collect JSONB parameters.
    pub fn compile(&mut self, filter: &MetadataFilter) -> AppResult<String> {
        self.compile_inner(filter, 0)
    }

    fn compile_inner(&mut self, filter: &MetadataFilter, depth: usize) -> AppResult<String> {
        if depth >= Self::MAX_DEPTH {
            return Err(AppError::BadRequest(
                "Filter nesting too deep (max 32 levels)".to_string(),
            ));
        }
        match filter {
            MetadataFilter::Leaf { key, value } => {
                let idx = self.next_param;
                self.next_param += 1;
                let json_obj = serde_json::json!({ key: value });
                self.params.push(json_obj);
                Ok(format!("metadata @> ${}::jsonb", idx))
            }
            MetadataFilter::And { and } => {
                if and.is_empty() {
                    return Ok("TRUE".to_string());
                }
                let parts: Vec<String> = and
                    .iter()
                    .map(|f| self.compile_inner(f, depth + 1))
                    .collect::<AppResult<_>>()?;
                Ok(format!("({})", parts.join(" AND ")))
            }
            MetadataFilter::Or { or } => {
                if or.is_empty() {
                    return Ok("FALSE".to_string());
                }
                let parts: Vec<String> = or
                    .iter()
                    .map(|f| self.compile_inner(f, depth + 1))
                    .collect::<AppResult<_>>()?;
                Ok(format!("({})", parts.join(" OR ")))
            }
            MetadataFilter::Not { not } => {
                let inner = self.compile_inner(not, depth + 1)?;
                Ok(format!("NOT ({})", inner))
            }
        }
    }

    /// Consume the compiler and return the collected JSONB parameters.
    pub fn into_params(self) -> Vec<serde_json::Value> {
        self.params
    }
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub results: Vec<FileReference>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

impl FileReference {
    /// Search file references by metadata filters with pagination, scoped to a project.
    pub async fn search_by_metadata(
        pool: &PgPool,
        project_id: Uuid,
        filters: Option<&MetadataFilter>,
        page: i64,
        per_page: i64,
    ) -> AppResult<SearchResult> {
        let limit = per_page.clamp(1, 100);
        let offset = page.max(1).saturating_sub(1).saturating_mul(limit);

        // $1 = project_id
        let mut compiler = MetadataFilterCompiler::new(2);

        let filter_clause = match filters {
            Some(f) => compiler.compile(f)?,
            None => "TRUE".to_string(),
        };

        let jsonb_params = compiler.into_params();
        let limit_idx = jsonb_params.len() + 2;
        let offset_idx = limit_idx + 1;

        let count_sql = format!(
            "SELECT COUNT(*)::bigint FROM file_references WHERE project_id = $1 AND {}",
            filter_clause
        );

        let data_sql = format!(
            "SELECT * FROM file_references WHERE project_id = $1 AND {} ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
            filter_clause, limit_idx, offset_idx
        );

        // Build count query args
        use sqlx::Arguments;
        let mut count_args = sqlx::postgres::PgArguments::default();
        count_args.add(project_id).map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
        for param in &jsonb_params {
            count_args.add(param).map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
        }

        let count_row: (i64,) = sqlx::query_as_with(&count_sql, count_args)
            .fetch_one(pool)
            .await?;

        // Build data query args
        let mut data_args = sqlx::postgres::PgArguments::default();
        data_args.add(project_id).map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
        for param in &jsonb_params {
            data_args.add(param).map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
        }
        data_args.add(limit).map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
        data_args.add(offset).map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;

        let rows: Vec<FileReference> = sqlx::query_as_with(&data_sql, data_args)
            .fetch_all(pool)
            .await?;

        Ok(SearchResult {
            results: rows,
            total: count_row.0,
            page: page.max(1),
            per_page: limit,
        })
    }

    /// Aggregate summary for file references matching metadata filters, scoped to a project.
    /// Joins with files table to get size data, groups by date for timeline.
    pub async fn search_summary(
        pool: &PgPool,
        project_id: Uuid,
        filters: Option<&MetadataFilter>,
    ) -> AppResult<SearchSummary> {
        // $1 = project_id
        let mut compiler = MetadataFilterCompiler::new(2);

        let filter_clause = match filters {
            Some(f) => compiler.compile(f)?,
            None => "TRUE".to_string(),
        };

        let jsonb_params = compiler.into_params();

        // Summary query: total files, total size, earliest and latest upload
        let summary_sql = format!(
            r#"SELECT
                COUNT(*)::bigint AS total_files,
                COALESCE(SUM(f.size), 0)::bigint AS total_size,
                MIN(fr.created_at) AS earliest_upload,
                MAX(fr.created_at) AS latest_upload
            FROM file_references fr
            JOIN files f ON f.id = fr.file_id
            WHERE fr.project_id = $1 AND {}"#,
            filter_clause
        );

        use sqlx::Arguments;
        let mut summary_args = sqlx::postgres::PgArguments::default();
        summary_args
            .add(project_id)
            .map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
        for param in &jsonb_params {
            summary_args
                .add(param)
                .map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
        }

        let summary_row: (i64, i64, Option<DateTime<Utc>>, Option<DateTime<Utc>>) =
            sqlx::query_as_with(&summary_sql, summary_args)
                .fetch_one(pool)
                .await?;

        // Timeline query: aggregate by day
        let timeline_sql = format!(
            r#"SELECT
                DATE(fr.created_at) AS date,
                COUNT(*)::bigint AS count,
                COALESCE(SUM(f.size), 0)::bigint AS size
            FROM file_references fr
            JOIN files f ON f.id = fr.file_id
            WHERE fr.project_id = $1 AND {}
            GROUP BY DATE(fr.created_at)
            ORDER BY DATE(fr.created_at) ASC"#,
            filter_clause
        );

        let mut timeline_args = sqlx::postgres::PgArguments::default();
        timeline_args
            .add(project_id)
            .map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
        for param in &jsonb_params {
            timeline_args
                .add(param)
                .map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
        }

        let timeline_rows: Vec<TimelineEntry> =
            sqlx::query_as_with(&timeline_sql, timeline_args)
                .fetch_all(pool)
                .await?;

        Ok(SearchSummary {
            total_files: summary_row.0,
            total_size: summary_row.1,
            earliest_upload: summary_row.2,
            latest_upload: summary_row.3,
            timeline: timeline_rows,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct SearchSummary {
    pub total_files: i64,
    pub total_size: i64,
    pub earliest_upload: Option<DateTime<Utc>>,
    pub latest_upload: Option<DateTime<Utc>>,
    pub timeline: Vec<TimelineEntry>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct TimelineEntry {
    pub date: chrono::NaiveDate,
    pub count: i64,
    pub size: i64,
}

// ─── Bulk Delete ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct BulkDeleteFilters {
    pub metadata_filters: Option<MetadataFilter>,
    pub created_before: Option<DateTime<Utc>>,
    pub created_after: Option<DateTime<Utc>>,
    pub size_min: Option<i64>,
    pub size_max: Option<i64>,
    pub last_accessed_before: Option<DateTime<Utc>>,
}

impl BulkDeleteFilters {
    pub fn has_any_filter(&self) -> bool {
        self.metadata_filters.is_some()
            || self.created_before.is_some()
            || self.created_after.is_some()
            || self.size_min.is_some()
            || self.size_max.is_some()
            || self.last_accessed_before.is_some()
    }
}

#[derive(Debug, Serialize)]
pub struct BulkDeletePreview {
    pub matching_references: i64,
    pub total_size: i64,
}

#[derive(Debug, Serialize)]
pub struct BulkDeleteResult {
    pub deleted_references: i64,
    pub orphaned_files_cleaned: i64,
    pub freed_bytes: i64,
}

/// Helper to build the WHERE clause and arguments for bulk delete queries.
/// Returns (where_clause, needs_files_join).
/// The caller must bind args in this order: project_id, then metadata params,
/// then created_before, created_after, size_min, size_max, last_accessed_before.
fn build_bulk_delete_clause(
    filters: &BulkDeleteFilters,
    start_param: usize,
) -> AppResult<(String, Vec<serde_json::Value>, bool, usize)> {
    let mut conditions = Vec::new();
    let mut needs_files_join = false;
    let mut param_idx = start_param;

    // Metadata filter
    let mut metadata_params = Vec::new();
    if let Some(ref mf) = filters.metadata_filters {
        let mut compiler = MetadataFilterCompiler::new(param_idx);
        let clause = compiler.compile(mf)?;
        conditions.push(clause);
        metadata_params = compiler.into_params();
        param_idx += metadata_params.len();
    }

    if filters.created_before.is_some() {
        conditions.push(format!("fr.created_at < ${}", param_idx));
        param_idx += 1;
    }

    if filters.created_after.is_some() {
        conditions.push(format!("fr.created_at > ${}", param_idx));
        param_idx += 1;
    }

    if filters.size_min.is_some() {
        conditions.push(format!("f.size >= ${}", param_idx));
        param_idx += 1;
        needs_files_join = true;
    }

    if filters.size_max.is_some() {
        conditions.push(format!("f.size <= ${}", param_idx));
        param_idx += 1;
        needs_files_join = true;
    }

    if filters.last_accessed_before.is_some() {
        conditions.push(format!(
            "NOT EXISTS (SELECT 1 FROM file_locations fl WHERE fl.file_id = fr.file_id AND (fl.last_accessed_at IS NULL OR fl.last_accessed_at >= ${}))",
            param_idx
        ));
        param_idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        "TRUE".to_string()
    } else {
        conditions.join(" AND ")
    };

    Ok((where_clause, metadata_params, needs_files_join, param_idx))
}

/// Bind all bulk delete filter parameters to a PgArguments instance.
fn bind_bulk_delete_args(
    args: &mut sqlx::postgres::PgArguments,
    project_id: Uuid,
    filters: &BulkDeleteFilters,
    metadata_params: &[serde_json::Value],
) -> AppResult<()> {
    use sqlx::Arguments;
    args.add(project_id)
        .map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
    for param in metadata_params {
        args.add(param)
            .map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
    }
    if let Some(ref v) = filters.created_before {
        args.add(v)
            .map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
    }
    if let Some(ref v) = filters.created_after {
        args.add(v)
            .map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
    }
    if let Some(ref v) = filters.size_min {
        args.add(v)
            .map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
    }
    if let Some(ref v) = filters.size_max {
        args.add(v)
            .map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
    }
    if let Some(ref v) = filters.last_accessed_before {
        args.add(v)
            .map_err(|e| AppError::Internal(format!("bind error: {}", e)))?;
    }
    Ok(())
}

impl FileReference {
    /// Preview bulk delete: returns count and total size of matching file references.
    pub async fn preview_bulk_delete(
        pool: &PgPool,
        project_id: Uuid,
        filters: &BulkDeleteFilters,
    ) -> AppResult<BulkDeletePreview> {
        let (where_clause, metadata_params, _needs_files_join, _next_idx) =
            build_bulk_delete_clause(filters, 2)?;

        // Always join files for total_size
        let sql = format!(
            r#"SELECT COUNT(*)::bigint, COALESCE(SUM(f.size), 0)::bigint
            FROM file_references fr
            JOIN files f ON f.id = fr.file_id
            WHERE fr.project_id = $1 AND {}"#,
            where_clause
        );

        let mut args = sqlx::postgres::PgArguments::default();
        bind_bulk_delete_args(&mut args, project_id, filters, &metadata_params)?;

        let row: (i64, i64) = sqlx::query_as_with(&sql, args)
            .fetch_one(pool)
            .await?;

        Ok(BulkDeletePreview {
            matching_references: row.0,
            total_size: row.1,
        })
    }

    /// Execute bulk delete: delete matching file references and return their file_ids.
    /// Returns (deleted_count, affected_file_ids).
    pub async fn execute_bulk_delete<'e, E>(
        executor: E,
        project_id: Uuid,
        filters: &BulkDeleteFilters,
    ) -> AppResult<(i64, Vec<Uuid>)>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let (where_clause, metadata_params, needs_files_join, _next_idx) =
            build_bulk_delete_clause(filters, 2)?;

        let subquery_from = if needs_files_join {
            "file_references fr JOIN files f ON f.id = fr.file_id"
        } else {
            "file_references fr"
        };

        let sql = format!(
            r#"DELETE FROM file_references WHERE id IN (
                SELECT fr.id FROM {}
                WHERE fr.project_id = $1 AND {}
            ) RETURNING file_id"#,
            subquery_from, where_clause
        );

        let mut args = sqlx::postgres::PgArguments::default();
        bind_bulk_delete_args(&mut args, project_id, filters, &metadata_params)?;

        let rows: Vec<(Uuid,)> = sqlx::query_as_with(&sql, args)
            .fetch_all(executor)
            .await?;

        let file_ids: Vec<Uuid> = rows.iter().map(|r| r.0).collect();
        let deleted_count = file_ids.len() as i64;

        Ok((deleted_count, file_ids))
    }
}

impl File {
    /// Find file IDs from the given list that have zero remaining file_references.
    pub async fn find_orphaned_from_ids<'e, E>(executor: E, file_ids: &[Uuid]) -> AppResult<Vec<Uuid>>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows: Vec<(Uuid,)> = sqlx::query_as(
            r#"SELECT f.id FROM files f
               WHERE f.id = ANY($1)
               AND NOT EXISTS (
                   SELECT 1 FROM file_references fr WHERE fr.file_id = f.id
               )"#,
        )
        .bind(file_ids)
        .fetch_all(executor)
        .await?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    }
}

// ─── FileLocation ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FileLocation {
    pub id: Uuid,
    pub file_id: Uuid,
    pub storage_id: Uuid,
    pub storage_path: String,
    pub status: String,
    pub synced_at: Option<DateTime<Utc>>,
    pub last_accessed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFileLocation {
    pub file_id: Uuid,
    pub storage_id: Uuid,
    pub storage_path: String,
    pub status: String,
}

impl FileLocation {
    pub async fn create(pool: &PgPool, input: &CreateFileLocation) -> AppResult<FileLocation> {
        // Set synced_at = NOW() when the initial status is 'synced'
        let row = if input.status == "synced" {
            sqlx::query_as::<_, FileLocation>(
                r#"INSERT INTO file_locations (file_id, storage_id, storage_path, status, synced_at)
                   VALUES ($1, $2, $3, $4, NOW())
                   RETURNING *"#,
            )
            .bind(input.file_id)
            .bind(input.storage_id)
            .bind(&input.storage_path)
            .bind(&input.status)
            .fetch_one(pool)
            .await?
        } else {
            sqlx::query_as::<_, FileLocation>(
                r#"INSERT INTO file_locations (file_id, storage_id, storage_path, status)
                   VALUES ($1, $2, $3, $4)
                   RETURNING *"#,
            )
            .bind(input.file_id)
            .bind(input.storage_id)
            .bind(&input.storage_path)
            .bind(&input.status)
            .fetch_one(pool)
            .await?
        };

        Ok(row)
    }

    /// Find file locations with synced status on enabled storages (for downloads).
    pub async fn find_for_file(pool: &PgPool, file_id: Uuid) -> AppResult<Vec<FileLocation>> {
        let rows = sqlx::query_as::<_, FileLocation>(
            r#"SELECT fl.* FROM file_locations fl
               JOIN storages s ON s.id = fl.storage_id
               WHERE fl.file_id = $1 AND s.enabled = TRUE AND fl.status = 'synced'
               ORDER BY s.is_hot DESC, fl.last_accessed_at DESC NULLS LAST"#,
        )
        .bind(file_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Find all file locations regardless of status (for metadata views).
    pub async fn find_all_for_file(pool: &PgPool, file_id: Uuid) -> AppResult<Vec<FileLocation>> {
        let rows = sqlx::query_as::<_, FileLocation>(
            r#"SELECT fl.* FROM file_locations fl
               WHERE fl.file_id = $1
               ORDER BY fl.created_at DESC"#,
        )
        .bind(file_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        status: &str,
    ) -> AppResult<FileLocation> {
        let synced_clause = if status == "synced" {
            ", synced_at = NOW()"
        } else {
            ""
        };

        let sql = format!(
            "UPDATE file_locations SET status = $1{} WHERE id = $2 RETURNING *",
            synced_clause
        );

        let row = sqlx::query_as::<_, FileLocation>(&sql)
            .bind(status)
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("File location {} not found", id)))?;

        Ok(row)
    }

    /// Update a file_location's status by file_id + storage_id (regardless of current status).
    /// Used by the sync worker when the location already exists (e.g. with 'archived' status).
    pub async fn update_status_by_file_and_storage(
        pool: &PgPool,
        file_id: Uuid,
        storage_id: Uuid,
        status: &str,
    ) -> AppResult<FileLocation> {
        let synced_clause = if status == "synced" {
            ", synced_at = NOW()"
        } else {
            ""
        };

        let sql = format!(
            "UPDATE file_locations SET status = $1{} WHERE file_id = $2 AND storage_id = $3 RETURNING *",
            synced_clause
        );

        let row = sqlx::query_as::<_, FileLocation>(&sql)
            .bind(status)
            .bind(file_id)
            .bind(storage_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "File location not found for file {} on storage {}",
                    file_id, storage_id
                ))
            })?;

        Ok(row)
    }

    pub async fn touch_accessed(pool: &PgPool, id: Uuid) -> AppResult<()> {
        sqlx::query("UPDATE file_locations SET last_accessed_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

// ─── SyncTask ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SyncTask {
    pub id: Uuid,
    pub file_id: Uuid,
    pub source_storage_id: Uuid,
    pub target_storage_id: Uuid,
    pub status: String,
    pub retries: i32,
    pub error_msg: Option<String>,
    pub retry_after: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSyncTask {
    pub file_id: Uuid,
    pub source_storage_id: Uuid,
    pub target_storage_id: Uuid,
}

impl SyncTask {
    pub async fn create(pool: &PgPool, input: &CreateSyncTask) -> AppResult<SyncTask> {
        let row = sqlx::query_as::<_, SyncTask>(
            r#"INSERT INTO sync_tasks (file_id, source_storage_id, target_storage_id)
               VALUES ($1, $2, $3)
               RETURNING *"#,
        )
        .bind(input.file_id)
        .bind(input.source_storage_id)
        .bind(input.target_storage_id)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn find_pending(pool: &PgPool, limit: i64) -> AppResult<Vec<SyncTask>> {
        let rows = sqlx::query_as::<_, SyncTask>(
            r#"SELECT * FROM sync_tasks
               WHERE status = 'pending'
               ORDER BY created_at ASC
               LIMIT $1"#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        status: &str,
        error_msg: Option<&str>,
    ) -> AppResult<SyncTask> {
        let row = sqlx::query_as::<_, SyncTask>(
            r#"UPDATE sync_tasks
               SET status = $1, error_msg = $2, retries = CASE WHEN $1 = 'failed' THEN retries + 1 ELSE retries END
               WHERE id = $3
               RETURNING *"#,
        )
        .bind(status)
        .bind(error_msg)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Sync task {} not found", id)))?;

        Ok(row)
    }

    /// Claim a batch of pending sync tasks using PostgreSQL advisory locks.
    /// Only returns tasks that this worker successfully locked.
    /// Tasks with retries >= max_retries are skipped and marked as 'failed'.
    ///
    /// Uses transaction-scoped advisory locks (`pg_try_advisory_xact_lock`) so that
    /// the lock and status update happen on the same connection. The lock auto-releases
    /// when the transaction commits, and the 'in_progress' status prevents other workers
    /// from re-claiming the task.
    pub async fn claim_pending(
        pool: &PgPool,
        limit: i64,
        max_retries: i32,
    ) -> AppResult<Vec<SyncTask>> {
        // Mark permanently failed tasks first
        sqlx::query(
            r#"UPDATE sync_tasks SET status = 'failed', error_msg = 'Max retries exceeded'
               WHERE status = 'pending' AND retries >= $1"#,
        )
        .bind(max_retries)
        .execute(pool)
        .await?;

        // Fetch pending tasks and try to lock each one with advisory lock
        // We use the UUID's first 8 bytes as the lock key
        let tasks = sqlx::query_as::<_, SyncTask>(
            r#"SELECT * FROM sync_tasks
               WHERE status = 'pending' AND retries < $1
               AND (retry_after IS NULL OR retry_after <= NOW())
               ORDER BY created_at ASC
               LIMIT $2"#,
        )
        .bind(max_retries)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let mut claimed = Vec::new();
        for task in tasks {
            // Use task UUID bytes as advisory lock key (two i32s from first 8 bytes)
            let id_bytes = task.id.as_bytes();
            let key1 = i32::from_le_bytes([id_bytes[0], id_bytes[1], id_bytes[2], id_bytes[3]]);
            let key2 = i32::from_le_bytes([id_bytes[4], id_bytes[5], id_bytes[6], id_bytes[7]]);

            // Use a transaction so the advisory lock and status update share the
            // same connection. pg_try_advisory_xact_lock auto-releases on commit.
            let mut tx = pool.begin().await?;

            let locked: (bool,) = sqlx::query_as(
                "SELECT pg_try_advisory_xact_lock($1, $2)",
            )
            .bind(key1)
            .bind(key2)
            .fetch_one(&mut *tx)
            .await?;

            if locked.0 {
                // Mark as in_progress within the same transaction
                sqlx::query(
                    "UPDATE sync_tasks SET status = 'in_progress' WHERE id = $1",
                )
                .bind(task.id)
                .execute(&mut *tx)
                .await?;

                tx.commit().await?;

                claimed.push(SyncTask {
                    status: "in_progress".to_string(),
                    ..task
                });
            } else {
                // Another worker claimed this task; rollback (releases xact lock)
                tx.rollback().await?;
            }
        }

        Ok(claimed)
    }

    /// Release the advisory lock for a sync task.
    ///
    /// This is now a no-op because `claim_pending` uses transaction-scoped locks
    /// (`pg_try_advisory_xact_lock`) that auto-release on commit. The 'in_progress'
    /// status prevents other workers from re-claiming the task.
    pub async fn release_lock(_pool: &PgPool, _task_id: Uuid) -> AppResult<()> {
        // Transaction-scoped advisory locks are automatically released when the
        // transaction in claim_pending commits. No explicit unlock needed.
        Ok(())
    }

    /// Re-queue a failed task back to pending for retry with exponential backoff.
    /// Delay: 2^retries seconds (1s, 2s, 4s, 8s, 16s, ..., capped at 5 minutes).
    pub async fn requeue_for_retry(
        pool: &PgPool,
        id: Uuid,
        error_msg: &str,
    ) -> AppResult<SyncTask> {
        let row = sqlx::query_as::<_, SyncTask>(
            r#"UPDATE sync_tasks
               SET status = 'pending',
                   error_msg = $1,
                   retries = retries + 1,
                   retry_after = NOW() + LEAST(
                       INTERVAL '1 second' * POWER(2, retries),
                       INTERVAL '5 minutes'
                   )
               WHERE id = $2
               RETURNING *"#,
        )
        .bind(error_msg)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Sync task {} not found", id)))?;

        Ok(row)
    }

    pub async fn list_filtered(
        pool: &PgPool,
        status: Option<&str>,
        storage_id: Option<Uuid>,
    ) -> AppResult<Vec<SyncTask>> {
        let mut sql = String::from("SELECT * FROM sync_tasks WHERE 1=1");
        let mut param_idx = 1u32;

        if status.is_some() {
            sql.push_str(&format!(" AND status = ${}", param_idx));
            param_idx += 1;
        }
        if storage_id.is_some() {
            sql.push_str(&format!(
                " AND (source_storage_id = ${p} OR target_storage_id = ${p})",
                p = param_idx
            ));
        }
        sql.push_str(" ORDER BY created_at DESC LIMIT 1000");

        let mut query = sqlx::query_as::<_, SyncTask>(&sql);
        if let Some(s) = status {
            query = query.bind(s);
        }
        if let Some(sid) = storage_id {
            query = query.bind(sid);
        }

        let rows = query.fetch_all(pool).await?;
        Ok(rows)
    }
}

// ─── Node ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Node {
    pub id: Uuid,
    pub node_id: String,
    pub address: String,
    pub started_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl Node {
    /// Register or update a node. Uses upsert to handle restarts.
    pub async fn register(
        pool: &PgPool,
        node_id: &str,
        address: &str,
    ) -> AppResult<Node> {
        let row = sqlx::query_as::<_, Node>(
            r#"INSERT INTO nodes (node_id, address, started_at, last_heartbeat)
               VALUES ($1, $2, NOW(), NOW())
               ON CONFLICT (node_id)
               DO UPDATE SET address = $2, started_at = NOW(), last_heartbeat = NOW()
               RETURNING *"#,
        )
        .bind(node_id)
        .bind(address)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Update the heartbeat timestamp for a node.
    pub async fn heartbeat(pool: &PgPool, node_id: &str) -> AppResult<()> {
        sqlx::query("UPDATE nodes SET last_heartbeat = NOW() WHERE node_id = $1")
            .bind(node_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// List nodes that have sent a heartbeat within the given threshold (seconds).
    pub async fn list_active(pool: &PgPool, heartbeat_threshold_secs: i64) -> AppResult<Vec<Node>> {
        let rows = sqlx::query_as::<_, Node>(
            r#"SELECT * FROM nodes
               WHERE last_heartbeat > NOW() - make_interval(secs => $1::double precision)
               ORDER BY started_at DESC"#,
        )
        .bind(heartbeat_threshold_secs as f64)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// List all registered nodes.
    pub async fn list_all(pool: &PgPool) -> AppResult<Vec<Node>> {
        let rows = sqlx::query_as::<_, Node>(
            "SELECT * FROM nodes ORDER BY started_at DESC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }
}

// ─── ProjectStorage ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProjectStorage {
    pub id: Uuid,
    pub project_id: Uuid,
    pub storage_id: Uuid,
    pub container_override: Option<String>,
    pub prefix_override: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ProjectStorageWithDetails {
    pub id: Uuid,
    pub project_id: Uuid,
    pub storage_id: Uuid,
    pub container_override: Option<String>,
    pub prefix_override: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub storage_name: String,
    pub storage_type: String,
    pub is_hot: bool,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateProjectStorage {
    pub storage_id: Uuid,
    pub container_override: Option<String>,
    pub prefix_override: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProjectStorage {
    pub container_override: Option<Option<String>>,
    pub prefix_override: Option<Option<String>>,
    pub is_active: Option<bool>,
}

impl ProjectStorage {
    pub async fn create(
        pool: &PgPool,
        project_id: Uuid,
        input: &CreateProjectStorage,
    ) -> AppResult<ProjectStorage> {
        let row = sqlx::query_as::<_, ProjectStorage>(
            r#"INSERT INTO project_storages (project_id, storage_id, container_override, prefix_override)
               VALUES ($1, $2, $3, $4)
               RETURNING *"#,
        )
        .bind(project_id)
        .bind(input.storage_id)
        .bind(&input.container_override)
        .bind(&input.prefix_override)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn list_for_project(
        pool: &PgPool,
        project_id: Uuid,
    ) -> AppResult<Vec<ProjectStorageWithDetails>> {
        let rows = sqlx::query_as::<_, ProjectStorageWithDetails>(
            r#"SELECT ps.id, ps.project_id, ps.storage_id,
                      ps.container_override, ps.prefix_override,
                      ps.is_active, ps.created_at, ps.updated_at,
                      s.name AS storage_name, s.storage_type,
                      s.is_hot, s.enabled
               FROM project_storages ps
               JOIN storages s ON s.id = ps.storage_id
               WHERE ps.project_id = $1
               ORDER BY s.is_hot DESC, s.name ASC"#,
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn update(
        pool: &PgPool,
        project_id: Uuid,
        storage_id: Uuid,
        input: &UpdateProjectStorage,
    ) -> AppResult<ProjectStorage> {
        let current = sqlx::query_as::<_, ProjectStorage>(
            "SELECT * FROM project_storages WHERE project_id = $1 AND storage_id = $2",
        )
        .bind(project_id)
        .bind(storage_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Project storage assignment not found".to_string()))?;

        let container_override = match &input.container_override {
            Some(v) => v.clone(),
            None => current.container_override,
        };
        let prefix_override = match &input.prefix_override {
            Some(v) => v.clone(),
            None => current.prefix_override,
        };
        let is_active = input.is_active.unwrap_or(current.is_active);

        let row = sqlx::query_as::<_, ProjectStorage>(
            r#"UPDATE project_storages
               SET container_override = $1, prefix_override = $2, is_active = $3
               WHERE project_id = $4 AND storage_id = $5
               RETURNING *"#,
        )
        .bind(&container_override)
        .bind(&prefix_override)
        .bind(is_active)
        .bind(project_id)
        .bind(storage_id)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn delete(pool: &PgPool, project_id: Uuid, storage_id: Uuid) -> AppResult<()> {
        let result = sqlx::query(
            "DELETE FROM project_storages WHERE project_id = $1 AND storage_id = $2",
        )
        .bind(project_id)
        .bind(storage_id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                "Project storage assignment not found".to_string(),
            ));
        }

        Ok(())
    }

    pub async fn list_available_storages(
        pool: &PgPool,
        project_id: Uuid,
    ) -> AppResult<Vec<Storage>> {
        let rows = sqlx::query_as::<_, Storage>(
            r#"SELECT s.* FROM storages s
               WHERE s.enabled = TRUE
               AND s.id NOT IN (
                   SELECT storage_id FROM project_storages WHERE project_id = $1
               )
               ORDER BY s.name ASC"#,
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Get a single assignment with container/prefix overrides.
    pub async fn find_for_project_and_storage(
        pool: &PgPool,
        project_id: Uuid,
        storage_id: Uuid,
    ) -> AppResult<Option<ProjectStorage>> {
        let row = sqlx::query_as::<_, ProjectStorage>(
            "SELECT * FROM project_storages WHERE project_id = $1 AND storage_id = $2",
        )
        .bind(project_id)
        .bind(storage_id)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }
}

// ─── User ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// User info with assignment date, returned by member list endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MemberInfo {
    pub id: Uuid,
    pub username: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub assigned_at: DateTime<Utc>,
    #[sqlx(default)]
    pub assignment_role: Option<String>,
}

#[derive(Debug)]
pub struct CreateUser {
    pub username: String,
    pub password_hash: String,
    pub role: String,
}

impl User {
    pub async fn create(pool: &PgPool, input: &CreateUser) -> AppResult<User> {
        let row = sqlx::query_as::<_, User>(
            r#"INSERT INTO users (username, password_hash, role)
               VALUES ($1, $2, $3)
               RETURNING *"#,
        )
        .bind(&input.username)
        .bind(&input.password_hash)
        .bind(&input.role)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> AppResult<User> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("User {} not found", id)))
    }

    pub async fn find_by_username(pool: &PgPool, username: &str) -> AppResult<Option<User>> {
        let row = sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    pub async fn list(pool: &PgPool) -> AppResult<Vec<User>> {
        let rows = sqlx::query_as::<_, User>(
            "SELECT * FROM users ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("User {} not found", id)));
        }
        Ok(())
    }

    pub async fn count(pool: &PgPool) -> AppResult<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(pool)
            .await?;
        Ok(count)
    }

    pub async fn update_role(pool: &PgPool, id: Uuid, role: &str) -> AppResult<User> {
        let row = sqlx::query_as::<_, User>(
            "UPDATE users SET role = $1, updated_at = NOW() WHERE id = $2 RETURNING *",
        )
        .bind(role)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User {} not found", id)))?;
        Ok(row)
    }

    pub async fn update_password_hash(pool: &PgPool, id: Uuid, password_hash: &str) -> AppResult<User> {
        let row = sqlx::query_as::<_, User>(
            "UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2 RETURNING *",
        )
        .bind(password_hash)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User {} not found", id)))?;
        Ok(row)
    }
}

// ─── RefreshToken ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    #[serde(skip_serializing)]
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct CreateRefreshToken {
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
}

impl RefreshToken {
    pub async fn create(pool: &PgPool, input: &CreateRefreshToken) -> AppResult<RefreshToken> {
        let row = sqlx::query_as::<_, RefreshToken>(
            r#"INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
               VALUES ($1, $2, $3)
               RETURNING *"#,
        )
        .bind(input.user_id)
        .bind(&input.token_hash)
        .bind(input.expires_at)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn find_by_hash(pool: &PgPool, token_hash: &str) -> AppResult<Option<RefreshToken>> {
        let row = sqlx::query_as::<_, RefreshToken>(
            "SELECT * FROM refresh_tokens WHERE token_hash = $1 AND expires_at > NOW()",
        )
        .bind(token_hash)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    pub async fn delete_by_user_id(pool: &PgPool, user_id: Uuid) -> AppResult<()> {
        sqlx::query("DELETE FROM refresh_tokens WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Atomically delete a refresh token by hash and return it.
    /// Returns None if the token was already consumed or doesn't exist.
    pub async fn consume_by_hash(pool: &PgPool, token_hash: &str) -> AppResult<Option<RefreshToken>> {
        let row = sqlx::query_as::<_, RefreshToken>(
            "DELETE FROM refresh_tokens WHERE token_hash = $1 AND expires_at > NOW() RETURNING *",
        )
        .bind(token_hash)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    pub async fn delete_by_hash(pool: &PgPool, token_hash: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = $1")
            .bind(token_hash)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete_expired(pool: &PgPool) -> AppResult<u64> {
        let result = sqlx::query("DELETE FROM refresh_tokens WHERE expires_at <= NOW()")
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }
}

// ─── UserProject ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserProject {
    pub id: Uuid,
    pub user_id: Uuid,
    pub project_id: Uuid,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

impl UserProject {
    pub async fn create(pool: &PgPool, user_id: Uuid, project_id: Uuid, role: &str) -> AppResult<UserProject> {
        let row = sqlx::query_as::<_, UserProject>(
            r#"INSERT INTO user_projects (user_id, project_id, role)
               VALUES ($1, $2, $3)
               RETURNING *"#,
        )
        .bind(user_id)
        .bind(project_id)
        .bind(role)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            if is_unique_violation(&e) {
                AppError::Conflict("User is already assigned to this project".to_string())
            } else {
                AppError::Database(e)
            }
        })?;

        Ok(row)
    }

    pub async fn list_for_project(pool: &PgPool, project_id: Uuid) -> AppResult<Vec<MemberInfo>> {
        let rows = sqlx::query_as::<_, MemberInfo>(
            r#"SELECT u.id, u.username, u.role, u.created_at, u.updated_at,
                      up.created_at AS assigned_at, up.role AS assignment_role
               FROM users u
               JOIN user_projects up ON up.user_id = u.id
               WHERE up.project_id = $1
               ORDER BY u.username ASC"#,
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> AppResult<Vec<ProjectAssignment>> {
        let rows = sqlx::query_as::<_, ProjectAssignment>(
            r#"SELECT p.id, p.name, p.slug, p.hot_to_cold_days, p.owner_id,
                      p.created_at, p.updated_at, up.role AS assignment_role
               FROM projects p
               JOIN user_projects up ON up.project_id = p.id
               WHERE up.user_id = $1 AND p.deleted_at IS NULL
               ORDER BY p.created_at DESC"#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn delete(pool: &PgPool, user_id: Uuid, project_id: Uuid) -> AppResult<()> {
        let result = sqlx::query(
            "DELETE FROM user_projects WHERE user_id = $1 AND project_id = $2",
        )
        .bind(user_id)
        .bind(project_id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                "User project assignment not found".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn update_role(pool: &PgPool, user_id: Uuid, project_id: Uuid, role: &str) -> AppResult<UserProject> {
        let row = sqlx::query_as::<_, UserProject>(
            r#"UPDATE user_projects SET role = $3
               WHERE user_id = $1 AND project_id = $2
               RETURNING *"#,
        )
        .bind(user_id)
        .bind(project_id)
        .bind(role)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("User project assignment not found".to_string()))?;

        Ok(row)
    }

    pub async fn get_role(pool: &PgPool, user_id: Uuid, project_id: Uuid) -> AppResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT role FROM user_projects WHERE user_id = $1 AND project_id = $2",
        )
        .bind(user_id)
        .bind(project_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| r.0))
    }

    pub async fn is_member(pool: &PgPool, user_id: Uuid, project_id: Uuid) -> AppResult<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM user_projects WHERE user_id = $1 AND project_id = $2)",
        )
        .bind(user_id)
        .bind(project_id)
        .fetch_one(pool)
        .await?;

        Ok(row.0)
    }
}

// ─── UserStorage ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserStorage {
    pub id: Uuid,
    pub user_id: Uuid,
    pub storage_id: Uuid,
    pub created_at: DateTime<Utc>,
}

impl UserStorage {
    pub async fn create(pool: &PgPool, user_id: Uuid, storage_id: Uuid) -> AppResult<UserStorage> {
        let row = sqlx::query_as::<_, UserStorage>(
            r#"INSERT INTO user_storages (user_id, storage_id)
               VALUES ($1, $2)
               RETURNING *"#,
        )
        .bind(user_id)
        .bind(storage_id)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            if is_unique_violation(&e) {
                AppError::Conflict("User is already assigned to this storage".to_string())
            } else {
                AppError::Database(e)
            }
        })?;

        Ok(row)
    }

    pub async fn list_for_storage(pool: &PgPool, storage_id: Uuid) -> AppResult<Vec<MemberInfo>> {
        let rows = sqlx::query_as::<_, MemberInfo>(
            r#"SELECT u.id, u.username, u.role, u.created_at, u.updated_at,
                      us.created_at AS assigned_at
               FROM users u
               JOIN user_storages us ON us.user_id = u.id
               WHERE us.storage_id = $1
               ORDER BY u.username ASC"#,
        )
        .bind(storage_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> AppResult<Vec<Storage>> {
        let rows = sqlx::query_as::<_, Storage>(
            r#"SELECT s.* FROM storages s
               JOIN user_storages us ON us.storage_id = s.id
               WHERE us.user_id = $1
               ORDER BY s.created_at DESC"#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn delete(pool: &PgPool, user_id: Uuid, storage_id: Uuid) -> AppResult<()> {
        let result = sqlx::query(
            "DELETE FROM user_storages WHERE user_id = $1 AND storage_id = $2",
        )
        .bind(user_id)
        .bind(storage_id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                "User storage assignment not found".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn is_member(pool: &PgPool, user_id: Uuid, storage_id: Uuid) -> AppResult<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM user_storages WHERE user_id = $1 AND storage_id = $2)",
        )
        .bind(user_id)
        .bind(storage_id)
        .fetch_one(pool)
        .await?;

        Ok(row.0)
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────────

pub fn is_unique_violation(e: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = e {
        // PostgreSQL unique_violation error code
        return db_err.code().as_deref() == Some("23505");
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_project_struct() {
        let input = CreateProject {
            name: "Test Project".to_string(),
            slug: "test-project".to_string(),
            hot_to_cold_days: Some(30),
        };
        assert_eq!(input.name, "Test Project");
        assert_eq!(input.slug, "test-project");
        assert_eq!(input.hot_to_cold_days, Some(30));
    }

    #[test]
    fn test_create_storage_struct() {
        let input = CreateStorage {
            name: "S3 Primary".to_string(),
            storage_type: "s3".to_string(),
            config: serde_json::json!({"bucket": "my-bucket", "region": "us-east-1"}),
            is_hot: Some(true),
            project_id: None,
            enabled: None,
        };
        assert_eq!(input.storage_type, "s3");
        assert_eq!(input.config["bucket"], "my-bucket");
    }

    #[test]
    fn test_create_file_struct() {
        let input = CreateFile {
            hash_sha256: "a".repeat(64),
            size: 1024,
            content_type: "text/plain".to_string(),
        };
        assert_eq!(input.hash_sha256.len(), 64);
        assert_eq!(input.size, 1024);
    }

    #[test]
    fn test_project_serialization() {
        let now = Utc::now();
        let project = Project {
            id: Uuid::new_v4(),
            name: "My Project".to_string(),
            slug: "my-project".to_string(),
            hot_to_cold_days: Some(7),
            owner_id: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };

        let json = serde_json::to_value(&project).unwrap();
        assert_eq!(json["name"], "My Project");
        assert_eq!(json["slug"], "my-project");
        assert_eq!(json["hot_to_cold_days"], 7);
    }

    #[test]
    fn test_storage_serialization() {
        let now = Utc::now();
        let storage = Storage {
            id: Uuid::new_v4(),
            name: "Local Disk".to_string(),
            storage_type: "local".to_string(),
            config: serde_json::json!({"path": "/data"}),
            is_hot: true,
            project_id: None,
            enabled: true,
            created_at: now,
            updated_at: now,
        };

        let json = serde_json::to_value(&storage).unwrap();
        assert_eq!(json["storage_type"], "local");
        assert!(json["is_hot"].as_bool().unwrap());
        assert!(json["project_id"].is_null());
    }

    #[test]
    fn test_file_serialization() {
        let now = Utc::now();
        let file = File {
            id: Uuid::new_v4(),
            hash_sha256: "abcd".repeat(16),
            size: 2048,
            content_type: "image/png".to_string(),
            created_at: now,
        };

        let json = serde_json::to_value(&file).unwrap();
        assert_eq!(json["size"], 2048);
        assert_eq!(json["content_type"], "image/png");
    }

    #[test]
    fn test_file_reference_serialization() {
        let now = Utc::now();
        let fref = FileReference {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            original_name: "photo.jpg".to_string(),
            metadata: serde_json::json!({"env": "prod"}),
            created_at: now,
        };

        let json = serde_json::to_value(&fref).unwrap();
        assert_eq!(json["original_name"], "photo.jpg");
        assert_eq!(json["metadata"]["env"], "prod");
    }

    #[test]
    fn test_file_reference_metadata_defaults_to_empty_object() {
        let json = serde_json::json!({
            "file_id": Uuid::new_v4(),
            "project_id": Uuid::new_v4(),
            "original_name": "test.txt"
        });
        let input: CreateFileReference = serde_json::from_value(json).unwrap();
        assert_eq!(input.metadata, serde_json::json!({}));
    }

    #[test]
    fn test_file_reference_metadata_with_values() {
        let now = Utc::now();
        let metadata = serde_json::json!({"env": "prod", "version": "1.0", "active": true, "priority": 5});
        let fref = FileReference {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            original_name: "report.pdf".to_string(),
            metadata: metadata.clone(),
            created_at: now,
        };

        let json = serde_json::to_value(&fref).unwrap();
        assert_eq!(json["metadata"]["env"], "prod");
        assert_eq!(json["metadata"]["version"], "1.0");
        assert_eq!(json["metadata"]["active"], true);
        assert_eq!(json["metadata"]["priority"], 5);
    }

    #[test]
    fn test_file_reference_empty_metadata_serialization() {
        let now = Utc::now();
        let fref = FileReference {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            original_name: "empty.txt".to_string(),
            metadata: serde_json::json!({}),
            created_at: now,
        };

        let json = serde_json::to_value(&fref).unwrap();
        assert_eq!(json["metadata"], serde_json::json!({}));
    }

    #[test]
    fn test_create_file_reference_with_metadata() {
        let metadata = serde_json::json!({"department": "engineering", "confidential": false});
        let input = CreateFileReference {
            file_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            original_name: "doc.txt".to_string(),
            metadata: metadata.clone(),
        };
        assert_eq!(input.metadata, metadata);
    }

    // ─── Metadata filter/search tests ───────────────────────────────────────

    #[test]
    fn test_metadata_filter_leaf_deserialization() {
        let json = serde_json::json!({"key": "env", "value": "prod"});
        let filter: MetadataFilter = serde_json::from_value(json).unwrap();
        assert_eq!(
            filter,
            MetadataFilter::Leaf {
                key: "env".to_string(),
                value: serde_json::json!("prod"),
            }
        );
    }

    #[test]
    fn test_metadata_filter_and_deserialization() {
        let json = serde_json::json!({
            "and": [
                {"key": "env", "value": "prod"},
                {"key": "version", "value": "2.0"}
            ]
        });
        let filter: MetadataFilter = serde_json::from_value(json).unwrap();
        match filter {
            MetadataFilter::And { and } => {
                assert_eq!(and.len(), 2);
            }
            _ => panic!("Expected And variant"),
        }
    }

    #[test]
    fn test_metadata_filter_or_deserialization() {
        let json = serde_json::json!({
            "or": [
                {"key": "env", "value": "prod"},
                {"key": "env", "value": "staging"}
            ]
        });
        let filter: MetadataFilter = serde_json::from_value(json).unwrap();
        match filter {
            MetadataFilter::Or { or } => {
                assert_eq!(or.len(), 2);
            }
            _ => panic!("Expected Or variant"),
        }
    }

    #[test]
    fn test_metadata_filter_not_deserialization() {
        let json = serde_json::json!({
            "not": {"key": "status", "value": "deprecated"}
        });
        let filter: MetadataFilter = serde_json::from_value(json).unwrap();
        match filter {
            MetadataFilter::Not { not } => {
                assert_eq!(
                    *not,
                    MetadataFilter::Leaf {
                        key: "status".to_string(),
                        value: serde_json::json!("deprecated"),
                    }
                );
            }
            _ => panic!("Expected Not variant"),
        }
    }

    #[test]
    fn test_metadata_filter_nested_and_or_not() {
        let json = serde_json::json!({
            "and": [
                {"key": "env", "value": "prod"},
                {"not": {"key": "status", "value": "deprecated"}},
                {"or": [
                    {"key": "tier", "value": "hot"},
                    {"key": "tier", "value": "warm"}
                ]}
            ]
        });
        let filter: MetadataFilter = serde_json::from_value(json).unwrap();
        match filter {
            MetadataFilter::And { and } => {
                assert_eq!(and.len(), 3);
            }
            _ => panic!("Expected And variant"),
        }
    }

    #[test]
    fn test_metadata_filter_compiler_leaf() {
        let filter = MetadataFilter::Leaf {
            key: "env".to_string(),
            value: serde_json::json!("prod"),
        };
        let mut compiler = MetadataFilterCompiler::new(2);
        let sql = compiler.compile(&filter).unwrap();
        assert_eq!(sql, "metadata @> $2::jsonb");
        let params = compiler.into_params();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], serde_json::json!({"env": "prod"}));
    }

    #[test]
    fn test_metadata_filter_compiler_and() {
        let filter = MetadataFilter::And {
            and: vec![
                MetadataFilter::Leaf {
                    key: "env".to_string(),
                    value: serde_json::json!("prod"),
                },
                MetadataFilter::Leaf {
                    key: "version".to_string(),
                    value: serde_json::json!("2.0"),
                },
            ],
        };
        let mut compiler = MetadataFilterCompiler::new(2);
        let sql = compiler.compile(&filter).unwrap();
        assert_eq!(sql, "(metadata @> $2::jsonb AND metadata @> $3::jsonb)");
        assert_eq!(compiler.into_params().len(), 2);
    }

    #[test]
    fn test_metadata_filter_compiler_or() {
        let filter = MetadataFilter::Or {
            or: vec![
                MetadataFilter::Leaf {
                    key: "env".to_string(),
                    value: serde_json::json!("prod"),
                },
                MetadataFilter::Leaf {
                    key: "env".to_string(),
                    value: serde_json::json!("staging"),
                },
            ],
        };
        let mut compiler = MetadataFilterCompiler::new(2);
        let sql = compiler.compile(&filter).unwrap();
        assert_eq!(sql, "(metadata @> $2::jsonb OR metadata @> $3::jsonb)");
    }

    #[test]
    fn test_metadata_filter_compiler_not() {
        let filter = MetadataFilter::Not {
            not: Box::new(MetadataFilter::Leaf {
                key: "status".to_string(),
                value: serde_json::json!("deprecated"),
            }),
        };
        let mut compiler = MetadataFilterCompiler::new(2);
        let sql = compiler.compile(&filter).unwrap();
        assert_eq!(sql, "NOT (metadata @> $2::jsonb)");
    }

    #[test]
    fn test_metadata_filter_compiler_nested() {
        let filter = MetadataFilter::And {
            and: vec![
                MetadataFilter::Leaf {
                    key: "env".to_string(),
                    value: serde_json::json!("prod"),
                },
                MetadataFilter::Not {
                    not: Box::new(MetadataFilter::Leaf {
                        key: "status".to_string(),
                        value: serde_json::json!("deprecated"),
                    }),
                },
                MetadataFilter::Or {
                    or: vec![
                        MetadataFilter::Leaf {
                            key: "tier".to_string(),
                            value: serde_json::json!("hot"),
                        },
                        MetadataFilter::Leaf {
                            key: "tier".to_string(),
                            value: serde_json::json!("warm"),
                        },
                    ],
                },
            ],
        };
        let mut compiler = MetadataFilterCompiler::new(2);
        let sql = compiler.compile(&filter).unwrap();
        assert_eq!(
            sql,
            "(metadata @> $2::jsonb AND NOT (metadata @> $3::jsonb) AND (metadata @> $4::jsonb OR metadata @> $5::jsonb))"
        );
        assert_eq!(compiler.into_params().len(), 4);
    }

    #[test]
    fn test_metadata_filter_compiler_empty_and() {
        let filter = MetadataFilter::And { and: vec![] };
        let mut compiler = MetadataFilterCompiler::new(2);
        let sql = compiler.compile(&filter).unwrap();
        assert_eq!(sql, "TRUE");
        assert_eq!(compiler.into_params().len(), 0);
    }

    #[test]
    fn test_metadata_filter_compiler_empty_or() {
        let filter = MetadataFilter::Or { or: vec![] };
        let mut compiler = MetadataFilterCompiler::new(2);
        let sql = compiler.compile(&filter).unwrap();
        assert_eq!(sql, "FALSE");
    }

    #[test]
    fn test_search_result_serialization() {
        let now = Utc::now();
        let result = SearchResult {
            results: vec![FileReference {
                id: Uuid::new_v4(),
                file_id: Uuid::new_v4(),
                project_id: Uuid::new_v4(),
                original_name: "test.txt".to_string(),
                metadata: serde_json::json!({"env": "prod"}),
                created_at: now,
            }],
            total: 1,
            page: 1,
            per_page: 50,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["total"], 1);
        assert_eq!(json["page"], 1);
        assert_eq!(json["per_page"], 50);
        assert_eq!(json["results"].as_array().unwrap().len(), 1);
        assert_eq!(json["results"][0]["metadata"]["env"], "prod");
    }

    #[test]
    fn test_search_summary_serialization() {
        let summary = SearchSummary {
            total_files: 42,
            total_size: 1048576,
            earliest_upload: Some(Utc::now()),
            latest_upload: Some(Utc::now()),
            timeline: vec![
                TimelineEntry {
                    date: chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
                    count: 10,
                    size: 524288,
                },
                TimelineEntry {
                    date: chrono::NaiveDate::from_ymd_opt(2026, 1, 16).unwrap(),
                    count: 32,
                    size: 524288,
                },
            ],
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["total_files"], 42);
        assert_eq!(json["total_size"], 1048576);
        assert!(!json["earliest_upload"].is_null());
        assert!(!json["latest_upload"].is_null());
        let timeline = json["timeline"].as_array().unwrap();
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[0]["count"], 10);
        assert_eq!(timeline[0]["size"], 524288);
        assert_eq!(timeline[0]["date"], "2026-01-15");
        assert_eq!(timeline[1]["count"], 32);
        assert_eq!(timeline[1]["date"], "2026-01-16");
    }

    #[test]
    fn test_search_summary_empty_results() {
        let summary = SearchSummary {
            total_files: 0,
            total_size: 0,
            earliest_upload: None,
            latest_upload: None,
            timeline: vec![],
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["total_files"], 0);
        assert_eq!(json["total_size"], 0);
        assert!(json["earliest_upload"].is_null());
        assert!(json["latest_upload"].is_null());
        assert_eq!(json["timeline"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_timeline_entry_serialization() {
        let entry = TimelineEntry {
            date: chrono::NaiveDate::from_ymd_opt(2026, 3, 14).unwrap(),
            count: 5,
            size: 2048,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["date"], "2026-03-14");
        assert_eq!(json["count"], 5);
        assert_eq!(json["size"], 2048);
    }

    // ─── Bulk delete tests ─────────────────────────────────────────────────

    #[test]
    fn test_bulk_delete_filters_has_any_filter_all_none() {
        let filters = BulkDeleteFilters {
            metadata_filters: None,
            created_before: None,
            created_after: None,
            size_min: None,
            size_max: None,
            last_accessed_before: None,
        };
        assert!(!filters.has_any_filter());
    }

    #[test]
    fn test_bulk_delete_filters_has_any_filter_metadata() {
        let filters = BulkDeleteFilters {
            metadata_filters: Some(MetadataFilter::Leaf {
                key: "env".to_string(),
                value: serde_json::json!("prod"),
            }),
            created_before: None,
            created_after: None,
            size_min: None,
            size_max: None,
            last_accessed_before: None,
        };
        assert!(filters.has_any_filter());
    }

    #[test]
    fn test_bulk_delete_filters_has_any_filter_size() {
        let filters = BulkDeleteFilters {
            metadata_filters: None,
            created_before: None,
            created_after: None,
            size_min: Some(1024),
            size_max: None,
            last_accessed_before: None,
        };
        assert!(filters.has_any_filter());
    }

    #[test]
    fn test_bulk_delete_filters_has_any_filter_dates() {
        let filters = BulkDeleteFilters {
            metadata_filters: None,
            created_before: Some(Utc::now()),
            created_after: None,
            size_min: None,
            size_max: None,
            last_accessed_before: None,
        };
        assert!(filters.has_any_filter());
    }

    #[test]
    fn test_bulk_delete_filters_deserialization() {
        let json = serde_json::json!({
            "metadata_filters": {"key": "env", "value": "prod"},
            "created_before": "2026-06-01T00:00:00Z",
            "size_min": 1048576
        });
        let filters: BulkDeleteFilters = serde_json::from_value(json).unwrap();
        assert!(filters.metadata_filters.is_some());
        assert!(filters.created_before.is_some());
        assert_eq!(filters.size_min, Some(1048576));
        assert!(filters.created_after.is_none());
        assert!(filters.size_max.is_none());
        assert!(filters.last_accessed_before.is_none());
    }

    #[test]
    fn test_bulk_delete_filters_deserialization_empty() {
        let json = serde_json::json!({});
        let filters: BulkDeleteFilters = serde_json::from_value(json).unwrap();
        assert!(!filters.has_any_filter());
    }

    #[test]
    fn test_bulk_delete_preview_serialization() {
        let preview = BulkDeletePreview {
            matching_references: 42,
            total_size: 10485760,
        };
        let json = serde_json::to_value(&preview).unwrap();
        assert_eq!(json["matching_references"], 42);
        assert_eq!(json["total_size"], 10485760);
    }

    #[test]
    fn test_bulk_delete_result_serialization() {
        let result = BulkDeleteResult {
            deleted_references: 15,
            orphaned_files_cleaned: 8,
            freed_bytes: 5242880,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["deleted_references"], 15);
        assert_eq!(json["orphaned_files_cleaned"], 8);
        assert_eq!(json["freed_bytes"], 5242880);
    }

    #[test]
    fn test_build_bulk_delete_clause_metadata_only() {
        let filters = BulkDeleteFilters {
            metadata_filters: Some(MetadataFilter::Leaf {
                key: "env".to_string(),
                value: serde_json::json!("prod"),
            }),
            created_before: None,
            created_after: None,
            size_min: None,
            size_max: None,
            last_accessed_before: None,
        };
        let (clause, params, needs_join, _) = build_bulk_delete_clause(&filters, 2).unwrap();
        assert_eq!(clause, "metadata @> $2::jsonb");
        assert_eq!(params.len(), 1);
        assert!(!needs_join);
    }

    #[test]
    fn test_build_bulk_delete_clause_size_filters() {
        let filters = BulkDeleteFilters {
            metadata_filters: None,
            created_before: None,
            created_after: None,
            size_min: Some(1024),
            size_max: Some(10240),
            last_accessed_before: None,
        };
        let (clause, params, needs_join, _) = build_bulk_delete_clause(&filters, 2).unwrap();
        assert_eq!(clause, "f.size >= $2 AND f.size <= $3");
        assert!(params.is_empty());
        assert!(needs_join);
    }

    #[test]
    fn test_build_bulk_delete_clause_combined() {
        let filters = BulkDeleteFilters {
            metadata_filters: Some(MetadataFilter::Leaf {
                key: "env".to_string(),
                value: serde_json::json!("prod"),
            }),
            created_before: Some(Utc::now()),
            created_after: None,
            size_min: Some(1024),
            size_max: None,
            last_accessed_before: Some(Utc::now()),
        };
        let (clause, params, needs_join, next_idx) =
            build_bulk_delete_clause(&filters, 2).unwrap();
        // $2 = metadata, $3 = created_before, $4 = size_min, $5 = last_accessed_before
        assert!(clause.contains("metadata @> $2::jsonb"));
        assert!(clause.contains("fr.created_at < $3"));
        assert!(clause.contains("f.size >= $4"));
        assert!(clause.contains("fl.last_accessed_at >= $5"));
        assert_eq!(params.len(), 1);
        assert!(needs_join);
        assert_eq!(next_idx, 6);
    }

    #[test]
    fn test_build_bulk_delete_clause_last_accessed_before() {
        let filters = BulkDeleteFilters {
            metadata_filters: None,
            created_before: None,
            created_after: None,
            size_min: None,
            size_max: None,
            last_accessed_before: Some(Utc::now()),
        };
        let (clause, _, needs_join, _) = build_bulk_delete_clause(&filters, 2).unwrap();
        assert!(clause.contains("NOT EXISTS"));
        assert!(clause.contains("fl.last_accessed_at >= $2"));
        assert!(!needs_join);
    }

    #[test]
    fn test_file_location_serialization() {
        let now = Utc::now();
        let loc = FileLocation {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            storage_id: Uuid::new_v4(),
            storage_path: "ab/cd/abcdef1234".to_string(),
            status: "synced".to_string(),
            synced_at: Some(now),
            last_accessed_at: Some(now),
            created_at: now,
        };

        let json = serde_json::to_value(&loc).unwrap();
        assert_eq!(json["status"], "synced");
        assert_eq!(json["storage_path"], "ab/cd/abcdef1234");
        assert!(!json["synced_at"].is_null());
    }

    #[test]
    fn test_sync_task_serialization() {
        let now = Utc::now();
        let task = SyncTask {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            source_storage_id: Uuid::new_v4(),
            target_storage_id: Uuid::new_v4(),
            status: "pending".to_string(),
            retries: 0,
            error_msg: None,
            retry_after: None,
            created_at: now,
            updated_at: now,
        };

        let json = serde_json::to_value(&task).unwrap();
        assert_eq!(json["status"], "pending");
        assert_eq!(json["retries"], 0);
        assert!(json["error_msg"].is_null());
    }

    #[test]
    fn test_sync_task_with_error() {
        let now = Utc::now();
        let task = SyncTask {
            id: Uuid::new_v4(),
            file_id: Uuid::new_v4(),
            source_storage_id: Uuid::new_v4(),
            target_storage_id: Uuid::new_v4(),
            status: "failed".to_string(),
            retries: 3,
            error_msg: Some("Connection timeout".to_string()),
            retry_after: None,
            created_at: now,
            updated_at: now,
        };

        let json = serde_json::to_value(&task).unwrap();
        assert_eq!(json["status"], "failed");
        assert_eq!(json["retries"], 3);
        assert_eq!(json["error_msg"], "Connection timeout");
    }

    #[test]
    fn test_update_project_partial() {
        let input = UpdateProject {
            name: Some("New Name".to_string()),
            slug: None,
            hot_to_cold_days: None,
        };
        assert_eq!(input.name, Some("New Name".to_string()));
        assert!(input.slug.is_none());
        assert!(input.hot_to_cold_days.is_none());
    }

    #[test]
    fn test_update_project_clear_hot_to_cold() {
        let input = UpdateProject {
            name: None,
            slug: None,
            hot_to_cold_days: Some(None),
        };
        assert_eq!(input.hot_to_cold_days, Some(None));
    }

    #[test]
    fn test_is_unique_violation() {
        // This tests the helper with a non-DB error
        let err = sqlx::Error::RowNotFound;
        assert!(!is_unique_violation(&err));
    }

    #[test]
    fn test_project_deserialization() {
        let json = serde_json::json!({
            "name": "Test",
            "slug": "test",
            "hot_to_cold_days": null
        });
        let input: CreateProject = serde_json::from_value(json).unwrap();
        assert_eq!(input.name, "Test");
        assert!(input.hot_to_cold_days.is_none());
    }

    #[test]
    fn test_storage_deserialization_defaults() {
        let json = serde_json::json!({
            "name": "My S3",
            "storage_type": "s3",
            "config": {"bucket": "test"}
        });
        let input: CreateStorage = serde_json::from_value(json).unwrap();
        assert!(input.is_hot.is_none());
        assert!(input.project_id.is_none());
        assert!(input.enabled.is_none());
    }

    #[test]
    fn test_node_serialization() {
        let now = Utc::now();
        let node = Node {
            id: Uuid::new_v4(),
            node_id: "node-abc-123".to_string(),
            address: "10.0.0.1:8080".to_string(),
            started_at: now,
            last_heartbeat: now,
            created_at: now,
        };

        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["node_id"], "node-abc-123");
        assert_eq!(json["address"], "10.0.0.1:8080");
        assert!(!json["started_at"].is_null());
        assert!(!json["last_heartbeat"].is_null());
    }

    #[test]
    fn test_user_serialization() {
        let now = Utc::now();
        let user = User {
            id: Uuid::new_v4(),
            username: "testuser".to_string(),
            password_hash: "secret_hash".to_string(),
            role: "admin".to_string(),
            created_at: now,
            updated_at: now,
        };

        let json = serde_json::to_value(&user).unwrap();
        assert_eq!(json["username"], "testuser");
        assert_eq!(json["role"], "admin");
        // password_hash should be skipped during serialization
        assert!(json.get("password_hash").is_none());
    }

    #[test]
    fn test_user_deserialization() {
        let json = serde_json::json!({
            "id": Uuid::new_v4(),
            "username": "alice",
            "password_hash": "hashed",
            "role": "user",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        });
        let user: User = serde_json::from_value(json).unwrap();
        assert_eq!(user.username, "alice");
        assert_eq!(user.role, "user");
        assert_eq!(user.password_hash, "hashed");
    }

    #[test]
    fn test_create_user_struct() {
        let input = CreateUser {
            username: "bob".to_string(),
            password_hash: "argon2hash".to_string(),
            role: "user".to_string(),
        };
        assert_eq!(input.username, "bob");
        assert_eq!(input.role, "user");
    }

    #[test]
    fn test_refresh_token_serialization() {
        let now = Utc::now();
        let token = RefreshToken {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            token_hash: "secret_token_hash".to_string(),
            expires_at: now,
            created_at: now,
        };

        let json = serde_json::to_value(&token).unwrap();
        assert!(!json["user_id"].is_null());
        assert!(!json["expires_at"].is_null());
        // token_hash should be skipped during serialization
        assert!(json.get("token_hash").is_none());
    }

    #[test]
    fn test_refresh_token_deserialization() {
        let user_id = Uuid::new_v4();
        let json = serde_json::json!({
            "id": Uuid::new_v4(),
            "user_id": user_id,
            "token_hash": "hash123",
            "expires_at": "2026-12-31T23:59:59Z",
            "created_at": "2026-01-01T00:00:00Z"
        });
        let token: RefreshToken = serde_json::from_value(json).unwrap();
        assert_eq!(token.user_id, user_id);
        assert_eq!(token.token_hash, "hash123");
    }

    #[test]
    fn test_create_refresh_token_struct() {
        let user_id = Uuid::new_v4();
        let input = CreateRefreshToken {
            user_id,
            token_hash: "hashed_token".to_string(),
            expires_at: Utc::now(),
        };
        assert_eq!(input.user_id, user_id);
        assert_eq!(input.token_hash, "hashed_token");
    }

    #[test]
    fn test_project_with_owner_id() {
        let now = Utc::now();
        let owner_id = Uuid::new_v4();
        let project = Project {
            id: Uuid::new_v4(),
            name: "Owned Project".to_string(),
            slug: "owned-project".to_string(),
            hot_to_cold_days: None,
            owner_id: Some(owner_id),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };

        let json = serde_json::to_value(&project).unwrap();
        assert_eq!(json["owner_id"], owner_id.to_string());
    }

    #[test]
    fn test_project_without_owner_id() {
        let now = Utc::now();
        let project = Project {
            id: Uuid::new_v4(),
            name: "Unowned Project".to_string(),
            slug: "unowned-project".to_string(),
            hot_to_cold_days: None,
            owner_id: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };

        let json = serde_json::to_value(&project).unwrap();
        assert!(json["owner_id"].is_null());
    }

    // ─── Integration tests that require a running PostgreSQL ───────────────────
    // Run with: DATABASE_URL=postgres://... cargo test -- --ignored

    #[ignore]
    #[tokio::test]
    async fn test_migration_applies() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Verify tables exist by querying information_schema
        let tables: Vec<(String,)> = sqlx::query_as(
            r#"SELECT table_name::text FROM information_schema.tables
               WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
               ORDER BY table_name"#,
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let table_names: Vec<&str> = tables.iter().map(|t| t.0.as_str()).collect();
        assert!(table_names.contains(&"projects"));
        assert!(table_names.contains(&"storages"));
        assert!(table_names.contains(&"files"));
        assert!(table_names.contains(&"file_references"));
        assert!(table_names.contains(&"file_locations"));
        assert!(table_names.contains(&"sync_tasks"));
        assert!(table_names.contains(&"nodes"));
        assert!(table_names.contains(&"users"));
        assert!(table_names.contains(&"refresh_tokens"));
    }

    #[ignore]
    #[tokio::test]
    async fn test_project_crud() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Create
        let input = CreateProject {
            name: "CRUD Test".to_string(),
            slug: format!("crud-test-{}", Uuid::new_v4()),
            hot_to_cold_days: Some(14),
        };
        let project = Project::create(&pool, &input, None).await.unwrap();
        assert_eq!(project.name, "CRUD Test");
        assert_eq!(project.hot_to_cold_days, Some(14));

        // Read
        let found = Project::find_by_id(&pool, project.id).await.unwrap();
        assert_eq!(found.id, project.id);

        // Update
        let update = UpdateProject {
            name: Some("Updated Name".to_string()),
            slug: None,
            hot_to_cold_days: None,
        };
        let updated = Project::update(&pool, project.id, &update).await.unwrap();
        assert_eq!(updated.name, "Updated Name");
        assert_eq!(updated.slug, project.slug);

        // List
        let all = Project::list(&pool).await.unwrap();
        assert!(all.iter().any(|p| p.id == project.id));

        // Delete
        Project::delete(&pool, project.id).await.unwrap();
        let result = Project::find_by_id(&pool, project.id).await;
        assert!(result.is_err());
    }

    #[ignore]
    #[tokio::test]
    async fn test_unique_constraints() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Project slug uniqueness
        let slug = format!("unique-test-{}", Uuid::new_v4());
        let input = CreateProject {
            name: "First".to_string(),
            slug: slug.clone(),
            hot_to_cold_days: None,
        };
        Project::create(&pool, &input, None).await.unwrap();

        let input2 = CreateProject {
            name: "Second".to_string(),
            slug,
            hot_to_cold_days: None,
        };
        let result = Project::create(&pool, &input2, None).await;
        assert!(result.is_err());

        // File hash uniqueness
        let hash = "b".repeat(64);
        let file_input = CreateFile {
            hash_sha256: hash.clone(),
            size: 100,
            content_type: "text/plain".to_string(),
        };
        File::create(&pool, &file_input).await.unwrap();

        // create_or_find should return existing file
        let (found, is_new) = File::create_or_find(&pool, &file_input).await.unwrap();
        assert!(!is_new);
        assert_eq!(found.hash_sha256.trim(), hash);
    }

    #[ignore]
    #[tokio::test]
    async fn test_node_registration() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let node_id = format!("test-node-{}", Uuid::new_v4());
        let address = "10.0.0.1:8080";

        // Register a new node
        let node = Node::register(&pool, &node_id, address).await.unwrap();
        assert_eq!(node.node_id, node_id);
        assert_eq!(node.address, address);

        // Re-register should upsert (update started_at and last_heartbeat)
        let node2 = Node::register(&pool, &node_id, "10.0.0.2:8080").await.unwrap();
        assert_eq!(node2.node_id, node_id);
        assert_eq!(node2.address, "10.0.0.2:8080");
        assert_eq!(node2.id, node.id); // Same row, updated

        // List active nodes (should include our node)
        let active = Node::list_active(&pool, 90).await.unwrap();
        assert!(active.iter().any(|n| n.node_id == node_id));
    }

    #[ignore]
    #[tokio::test]
    async fn test_node_heartbeat() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let node_id = format!("heartbeat-test-{}", Uuid::new_v4());

        // Register
        Node::register(&pool, &node_id, "10.0.0.5:8080").await.unwrap();

        // Heartbeat should succeed
        Node::heartbeat(&pool, &node_id).await.unwrap();

        // Verify the node is in the active list
        let active = Node::list_active(&pool, 90).await.unwrap();
        assert!(active.iter().any(|n| n.node_id == node_id));

        // List all should also include it
        let all = Node::list_all(&pool).await.unwrap();
        assert!(all.iter().any(|n| n.node_id == node_id));
    }

    #[test]
    fn test_user_project_serialization() {
        let now = Utc::now();
        let up = UserProject {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            role: "member".to_string(),
            created_at: now,
        };

        let json = serde_json::to_value(&up).unwrap();
        assert!(!json["user_id"].is_null());
        assert!(!json["project_id"].is_null());
        assert!(!json["created_at"].is_null());
    }

    #[test]
    fn test_user_storage_serialization() {
        let now = Utc::now();
        let us = UserStorage {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            storage_id: Uuid::new_v4(),
            created_at: now,
        };

        let json = serde_json::to_value(&us).unwrap();
        assert!(!json["user_id"].is_null());
        assert!(!json["storage_id"].is_null());
        assert!(!json["created_at"].is_null());
    }

    #[test]
    fn test_user_project_deserialization() {
        let user_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let json = serde_json::json!({
            "id": Uuid::new_v4(),
            "user_id": user_id,
            "project_id": project_id,
            "created_at": "2026-01-01T00:00:00Z"
        });
        let up: UserProject = serde_json::from_value(json).unwrap();
        assert_eq!(up.user_id, user_id);
        assert_eq!(up.project_id, project_id);
    }

    #[test]
    fn test_user_storage_deserialization() {
        let user_id = Uuid::new_v4();
        let storage_id = Uuid::new_v4();
        let json = serde_json::json!({
            "id": Uuid::new_v4(),
            "user_id": user_id,
            "storage_id": storage_id,
            "created_at": "2026-01-01T00:00:00Z"
        });
        let us: UserStorage = serde_json::from_value(json).unwrap();
        assert_eq!(us.user_id, user_id);
        assert_eq!(us.storage_id, storage_id);
    }

    #[ignore]
    #[tokio::test]
    async fn test_user_project_crud() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Create a user
        let user = User::create(
            &pool,
            &CreateUser {
                username: format!("up-test-{}", Uuid::new_v4()),
                password_hash: "hash".to_string(),
                role: "user".to_string(),
            },
        )
        .await
        .unwrap();

        // Create a project
        let project = Project::create(
            &pool,
            &CreateProject {
                name: "UP Test Project".to_string(),
                slug: format!("up-test-{}", Uuid::new_v4()),
                hot_to_cold_days: None,
            },
            None,
        )
        .await
        .unwrap();

        // Assign user to project
        let assignment = UserProject::create(&pool, user.id, project.id, "member").await.unwrap();
        assert_eq!(assignment.user_id, user.id);
        assert_eq!(assignment.project_id, project.id);
        assert_eq!(assignment.role, "member");

        // is_member should be true
        assert!(UserProject::is_member(&pool, user.id, project.id).await.unwrap());

        // list_for_project should include our user
        let users = UserProject::list_for_project(&pool, project.id).await.unwrap();
        assert!(users.iter().any(|u| u.id == user.id));

        // list_for_user should include our project
        let projects = UserProject::list_for_user(&pool, user.id).await.unwrap();
        assert!(projects.iter().any(|p| p.id == project.id));

        // Duplicate assignment should return Conflict
        let dup = UserProject::create(&pool, user.id, project.id, "member").await;
        assert!(dup.is_err());

        // Delete assignment
        UserProject::delete(&pool, user.id, project.id).await.unwrap();
        assert!(!UserProject::is_member(&pool, user.id, project.id).await.unwrap());

        // Delete non-existent assignment should error
        let del = UserProject::delete(&pool, user.id, project.id).await;
        assert!(del.is_err());
    }

    #[ignore]
    #[tokio::test]
    async fn test_user_storage_crud() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Create a user
        let user = User::create(
            &pool,
            &CreateUser {
                username: format!("us-test-{}", Uuid::new_v4()),
                password_hash: "hash".to_string(),
                role: "user".to_string(),
            },
        )
        .await
        .unwrap();

        // Create a storage
        let storage = Storage::create(
            &pool,
            &CreateStorage {
                name: format!("US Test Storage {}", Uuid::new_v4()),
                storage_type: "local".to_string(),
                config: serde_json::json!({"path": "/tmp/test"}),
                is_hot: Some(true),
                project_id: None,
                enabled: Some(true),
            },
        )
        .await
        .unwrap();

        // Assign user to storage
        let assignment = UserStorage::create(&pool, user.id, storage.id).await.unwrap();
        assert_eq!(assignment.user_id, user.id);
        assert_eq!(assignment.storage_id, storage.id);

        // is_member should be true
        assert!(UserStorage::is_member(&pool, user.id, storage.id).await.unwrap());

        // list_for_storage should include our user
        let users = UserStorage::list_for_storage(&pool, storage.id).await.unwrap();
        assert!(users.iter().any(|u| u.id == user.id));

        // list_for_user should include our storage
        let storages = UserStorage::list_for_user(&pool, user.id).await.unwrap();
        assert!(storages.iter().any(|s| s.id == storage.id));

        // Duplicate assignment should return Conflict
        let dup = UserStorage::create(&pool, user.id, storage.id).await;
        assert!(dup.is_err());

        // Delete assignment
        UserStorage::delete(&pool, user.id, storage.id).await.unwrap();
        assert!(!UserStorage::is_member(&pool, user.id, storage.id).await.unwrap());

        // Delete non-existent assignment should error
        let del = UserStorage::delete(&pool, user.id, storage.id).await;
        assert!(del.is_err());
    }

    #[ignore]
    #[tokio::test]
    async fn test_migration_007_tables_exist() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let tables: Vec<(String,)> = sqlx::query_as(
            r#"SELECT table_name::text FROM information_schema.tables
               WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
               ORDER BY table_name"#,
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let table_names: Vec<&str> = tables.iter().map(|t| t.0.as_str()).collect();
        assert!(table_names.contains(&"user_projects"));
        assert!(table_names.contains(&"user_storages"));
    }

    #[ignore]
    #[tokio::test]
    async fn test_file_reference_metadata_column() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Verify metadata column exists with correct default
        let col: (String, String, String) = sqlx::query_as(
            r#"SELECT column_name::text, data_type::text, column_default::text
               FROM information_schema.columns
               WHERE table_name = 'file_references' AND column_name = 'metadata'"#,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(col.0, "metadata");
        assert_eq!(col.1, "jsonb");
        assert!(col.2.contains("'{}'::jsonb"));

        // Verify GIN index exists
        let idx_exists: (bool,) = sqlx::query_as(
            r#"SELECT EXISTS(
                SELECT 1 FROM pg_indexes
                WHERE tablename = 'file_references' AND indexname = 'idx_file_references_metadata'
            )"#,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(idx_exists.0, "GIN index on metadata should exist");
    }

    #[ignore]
    #[tokio::test]
    async fn test_file_reference_metadata_crud() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required for DB tests");
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        // Create a project and file to reference
        let project = Project::create(
            &pool,
            &CreateProject {
                name: "Meta Test".to_string(),
                slug: format!("meta-test-{}", Uuid::new_v4()),
                hot_to_cold_days: None,
            },
            None,
        )
        .await
        .unwrap();

        let file_input = CreateFile {
            hash_sha256: format!("{:064x}", Uuid::new_v4().as_u128()),
            size: 512,
            content_type: "text/plain".to_string(),
        };
        let (file, _) = File::create_or_find(&pool, &file_input).await.unwrap();

        // Create reference without metadata (defaults to {})
        let create_ref = CreateFileReference {
            file_id: file.id,
            project_id: project.id,
            original_name: "no-meta.txt".to_string(),
            metadata: serde_json::json!({}),
        };
        let fref = FileReference::create(&pool, &create_ref).await.unwrap();
        assert_eq!(fref.metadata, serde_json::json!({}));

        // Create reference with metadata
        let meta = serde_json::json!({"env": "staging", "version": "2.0"});
        let create_ref2 = CreateFileReference {
            file_id: file.id,
            project_id: project.id,
            original_name: "with-meta.txt".to_string(),
            metadata: meta.clone(),
        };
        let fref2 = FileReference::create(&pool, &create_ref2).await.unwrap();
        assert_eq!(fref2.metadata, meta);

        // Update metadata
        let new_meta = serde_json::json!({"env": "prod", "version": "3.0", "release": true});
        let updated = FileReference::update_metadata(&pool, fref2.id, &new_meta)
            .await
            .unwrap();
        assert_eq!(updated.metadata, new_meta);

        // List for project should return metadata
        let listed = FileReference::list_for_project(&pool, project.id, 10, 0)
            .await
            .unwrap();
        assert!(listed.len() >= 2);
        let found = listed.iter().find(|r| r.id == fref2.id).unwrap();
        assert_eq!(found.metadata, new_meta);

        // Find by file_id should return metadata
        let by_file = FileReference::find_by_file_id(&pool, file.id).await.unwrap();
        assert!(by_file.len() >= 2);
    }
}
