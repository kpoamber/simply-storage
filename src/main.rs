use actix_web::{App, HttpServer, web};
use innovare_storage::config::AppConfig;
use innovare_storage::db::models::Node;
use innovare_storage::services::{BulkService, FileService, TierService};
use innovare_storage::storage::StorageRegistry;
use innovare_storage::workers::{SyncWorker, TierWorker};
use sqlx::PgPool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

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

    // Set up storage registry and load backends from database
    let registry = Arc::new(StorageRegistry::new());
    if let Err(e) = innovare_storage::storage::registry::load_backends_from_db(
        &pool,
        &registry,
        &config.storage.hmac_secret,
    )
    .await
    {
        tracing::warn!("Failed to load storage backends from DB: {}", e);
    }

    let file_service = FileService::new(
        pool.clone(),
        registry.clone(),
        config.storage.hmac_secret.clone(),
    );
    let tier_service = TierService::new(pool.clone(), registry.clone());
    let bulk_service = BulkService::new(pool.clone(), registry.clone());

    // Set up cancellation token for graceful shutdown of background workers
    let cancel_token = CancellationToken::new();

    // Spawn background sync workers
    let worker_handles = SyncWorker::spawn_workers(
        pool.clone(),
        registry.clone(),
        config.sync.clone(),
        config.storage.hmac_secret.clone(),
        cancel_token.clone(),
    );
    tracing::info!(
        num_workers = config.sync.num_workers,
        "Background sync workers started"
    );

    // Spawn background tier worker for hot/cold management
    let tier_handle = TierWorker::spawn(
        pool.clone(),
        registry.clone(),
        cancel_token.clone(),
        config.sync.tier_scan_interval_secs,
    );
    tracing::info!(
        scan_interval_secs = config.sync.tier_scan_interval_secs,
        "Tier worker started"
    );

    // Generate a unique node ID and register this instance
    let node_id = format!("node-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("unknown"));
    let node_address = format!("{}:{}", config.server.host, config.server.port);
    match Node::register(&pool, &node_id, &node_address).await {
        Ok(node) => tracing::info!(node_id = %node.node_id, address = %node.address, "Node registered"),
        Err(e) => tracing::warn!("Failed to register node: {}", e),
    }

    // Spawn heartbeat background task (every 30 seconds)
    let heartbeat_handle = spawn_heartbeat(pool.clone(), node_id.clone(), cancel_token.clone());
    tracing::info!(node_id = %node_id, "Heartbeat worker started (30s interval)");

    let bind_addr = format!("{}:{}", config.server.host, config.server.port);
    let config_data = web::Data::new(config);
    let pool_data = web::Data::new(pool);
    let registry_data = web::Data::new(registry);
    let file_service_data = web::Data::new(file_service);
    let tier_service_data = web::Data::new(tier_service);
    let bulk_service_data = web::Data::new(bulk_service);

    let server = HttpServer::new(move || {
        App::new()
            .app_data(config_data.clone())
            .app_data(pool_data.clone())
            .app_data(registry_data.clone())
            .app_data(file_service_data.clone())
            .app_data(tier_service_data.clone())
            .app_data(bulk_service_data.clone())
            .configure(innovare_storage::configure_app)
    })
    .bind(&bind_addr)?
    .run();

    // Run the server; when it stops, signal workers to shut down
    let result = server.await;

    tracing::info!("HTTP server stopped, shutting down background workers...");
    cancel_token.cancel();

    // Wait for all workers to finish
    for handle in worker_handles {
        let _ = handle.await;
    }
    let _ = tier_handle.await;
    let _ = heartbeat_handle.await;
    tracing::info!("All background workers stopped");

    result
}

fn spawn_heartbeat(
    pool: PgPool,
    node_id: String,
    cancel_token: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    tracing::info!(node_id = %node_id, "Heartbeat worker shutting down");
                    break;
                }
                _ = interval.tick() => {
                    if let Err(e) = Node::heartbeat(&pool, &node_id).await {
                        tracing::warn!(node_id = %node_id, "Heartbeat failed: {}", e);
                    }
                }
            }
        }
    })
}
