pub mod models;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::DatabaseConfig;
use crate::error::AppResult;

/// Create a PostgreSQL connection pool from the database configuration.
pub async fn create_pool(config: &DatabaseConfig) -> AppResult<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .connect(&config.url)
        .await
        .map_err(crate::error::AppError::Database)?;

    tracing::info!("Database connection pool established");
    Ok(pool)
}

/// Run pending database migrations.
pub async fn run_migrations(pool: &PgPool) -> AppResult<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| crate::error::AppError::Internal(format!("Migration failed: {}", e)))?;

    tracing::info!("Database migrations applied successfully");
    Ok(())
}

/// Try to configure Citus distribution. Non-fatal if Citus is not available.
pub async fn configure_citus(pool: &PgPool) {
    let distributions = [
        ("files", "id"),
        ("file_locations", "file_id"),
        ("file_references", "project_id"),
    ];

    for (table, column) in distributions {
        let sql = format!("SELECT create_distributed_table('{}', '{}')", table, column);
        match sqlx::query(&sql).execute(pool).await {
            Ok(_) => tracing::info!("Citus: distributed table {} by {}", table, column),
            Err(e) => tracing::debug!(
                "Citus distribution for {} skipped (not available or already configured): {}",
                table,
                e
            ),
        }
    }
}
