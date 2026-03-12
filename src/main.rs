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

    let bind_addr = format!("{}:{}", config.server.host, config.server.port);
    let config_data = web::Data::new(config);

    HttpServer::new(move || {
        App::new()
            .app_data(config_data.clone())
            .configure(innovare_storage::configure_routes)
    })
    .bind(&bind_addr)?
    .run()
    .await
}
