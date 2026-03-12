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
    // Local storage temp URL download endpoint (outside /api scope)
    cfg.route(
        "/download/local",
        web::get().to(api::files::download_local_temp),
    );
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

#[cfg(test)]
mod deployment_tests {
    #[test]
    fn test_dockerfile_exists_and_has_valid_structure() {
        let dockerfile = std::fs::read_to_string("Dockerfile").expect("Dockerfile should exist");

        // Verify multi-stage build structure
        assert!(dockerfile.contains("FROM node:"), "Should have frontend build stage");
        assert!(dockerfile.contains("FROM rust:"), "Should have Rust build stage");
        assert!(dockerfile.contains("FROM debian:"), "Should have runtime stage");

        // Verify key instructions
        assert!(dockerfile.contains("cargo build --release"), "Should build in release mode");
        assert!(dockerfile.contains("frontend/dist"), "Should copy frontend build output");
        assert!(dockerfile.contains("EXPOSE 8080"), "Should expose port 8080");
        assert!(dockerfile.contains("migrations/"), "Should include migrations");
    }

    #[test]
    fn test_docker_compose_exists_and_has_required_services() {
        let compose = std::fs::read_to_string("docker-compose.yml")
            .expect("docker-compose.yml should exist");

        assert!(compose.contains("nginx:"), "Should have nginx service");
        assert!(compose.contains("app:"), "Should have app service");
        assert!(compose.contains("postgres:"), "Should have postgres service");
        assert!(compose.contains("postgres-worker-1:"), "Should have Citus worker 1");
        assert!(compose.contains("postgres-worker-2:"), "Should have Citus worker 2");
        assert!(compose.contains("healthcheck:"), "Should have health checks");
    }

    #[test]
    fn test_nginx_conf_has_required_directives() {
        let nginx = std::fs::read_to_string("docker/nginx.conf")
            .expect("docker/nginx.conf should exist");

        assert!(nginx.contains("least_conn"), "Should use least_conn load balancing");
        assert!(nginx.contains("proxy_pass"), "Should have proxy_pass");
        assert!(nginx.contains("client_max_body_size"), "Should set client_max_body_size");
        assert!(nginx.contains("proxy_read_timeout"), "Should set proxy_read_timeout");
        assert!(nginx.contains("/health"), "Should have health check location");
        assert!(nginx.contains("ssl_certificate"), "Should have TLS support (commented)");
    }

    #[test]
    fn test_github_actions_workflow_exists() {
        let workflow = std::fs::read_to_string(".github/workflows/build-push.yml")
            .expect("GitHub Actions workflow should exist");

        assert!(workflow.contains("push:"), "Should trigger on push");
        assert!(workflow.contains("main"), "Should trigger on main branch");
        assert!(workflow.contains("ghcr.io"), "Should use GHCR");
        assert!(workflow.contains("docker/build-push-action"), "Should use build-push action");
        assert!(workflow.contains("GITHUB_TOKEN"), "Should use GITHUB_TOKEN for auth");
    }

    #[test]
    fn test_deploy_script_exists_and_has_join_support() {
        let script = std::fs::read_to_string("deploy/deploy.sh")
            .expect("deploy/deploy.sh should exist");

        assert!(script.contains("--join"), "Should support --join flag");
        assert!(script.contains("config-export"), "Should fetch config from existing node");
        assert!(script.contains("docker pull"), "Should pull Docker image");
        assert!(script.contains("docker run"), "Should start container");
        assert!(script.contains("health"), "Should check health after deploy");
    }

    #[test]
    fn test_cloud_init_template_exists() {
        let cloud_init = std::fs::read_to_string("deploy/cloud-init.yml")
            .expect("deploy/cloud-init.yml should exist");

        assert!(cloud_init.contains("#cloud-config"), "Should be valid cloud-config");
        assert!(cloud_init.contains("docker"), "Should install Docker");
        assert!(cloud_init.contains("ghcr.io"), "Should reference GHCR");
    }
}
