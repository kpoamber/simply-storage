use actix_web::{App, HttpServer, web};
use innovare_storage::config::AppConfig;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    innovare_storage::init_tracing();

    let config = AppConfig::load().unwrap_or_else(|e| {
        tracing::warn!("Failed to load config file, using defaults: {}", e);
        // Fall back to defaults when no config file exists
        AppConfig::load_from("__nonexistent__").expect("defaults should always work")
    });

    tracing::info!(
        "Starting server on {}:{}",
        config.server.host,
        config.server.port
    );

    // Set up database connection pool
    let pool = innovare_storage::db::create_pool(&config.database)
        .await
        .expect("Failed to create database pool");

    // Run migrations
    innovare_storage::db::run_migrations(&pool)
        .await
        .expect("Failed to run database migrations");

    // Optionally configure Citus distribution
    innovare_storage::db::configure_citus(&pool).await;

    let bind_addr = format!("{}:{}", config.server.host, config.server.port);
    let config_data = web::Data::new(config);
    let pool_data = web::Data::new(pool);

    HttpServer::new(move || {
        App::new()
            .app_data(config_data.clone())
            .app_data(pool_data.clone())
            .configure(innovare_storage::configure_routes)
    })
    .bind(&bind_addr)?
    .run()
    .await
}
