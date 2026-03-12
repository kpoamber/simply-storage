pub mod api;
pub mod config;
pub mod db;
pub mod error;
pub mod services;
pub mod storage;
pub mod workers;

use actix_files::NamedFile;
use actix_web::{web, HttpResponse};
use serde_json::json;
use std::path::PathBuf;

/// Directory where the frontend build output is located.
const FRONTEND_DIR: &str = "frontend/dist";

pub async fn health_check() -> HttpResponse {
    HttpResponse::Ok().json(json!({
        "status": "ok",
        "service": "innovare-storage",
    }))
}

/// SPA fallback: serve index.html for any path not matched by API or static files.
async fn spa_fallback() -> actix_web::Result<NamedFile> {
    let path = PathBuf::from(FRONTEND_DIR).join("index.html");
    Ok(NamedFile::open(path)?)
}

/// Configure API routes for the application (no static file serving).
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health_check));
    api::configure_api_routes(cfg);
}

/// Configure the full application including static file serving.
/// This should be used in main.rs to set up the App.
pub fn configure_app(cfg: &mut web::ServiceConfig) {
    configure_routes(cfg);

    // Serve frontend static files from frontend/dist/
    let frontend_path = PathBuf::from(FRONTEND_DIR);
    if frontend_path.exists() {
        cfg.service(
            actix_files::Files::new("/", FRONTEND_DIR)
                .index_file("index.html")
                .default_handler(web::get().to(spa_fallback)),
        );
    }
}

/// Initialize tracing/logging with tracing-subscriber.
pub fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};

    #[actix_rt::test]
    async fn test_health_check() {
        let app = test::init_service(
            App::new().configure(configure_routes),
        )
        .await;

        let req = test::TestRequest::get().uri("/health").to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), 200);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["service"], "innovare-storage");
    }

    #[actix_rt::test]
    async fn test_unknown_route_returns_404() {
        let app = test::init_service(
            App::new().configure(configure_routes),
        )
        .await;

        let req = test::TestRequest::get().uri("/nonexistent").to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), 404);
    }
}
