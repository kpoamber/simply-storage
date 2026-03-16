pub mod api;
pub mod config;
pub mod db;
pub mod error;
pub mod services;
pub mod storage;
pub mod workers;

/// Constant-time comparison to prevent timing attacks on HMAC signatures.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

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
    // Public shared link proxy endpoints (no auth required)
    api::shared_links::configure_public(cfg);
    api::configure_api_routes(cfg);
}

/// Configure the full application including static file serving.
/// This should be used in main.rs to set up the App.
pub fn configure_app(cfg: &mut web::ServiceConfig) {
    // Allow uploads up to 500MB
    cfg.app_data(actix_multipart::form::MultipartFormConfig::default().total_limit(500 * 1024 * 1024));
    cfg.app_data(actix_web::web::PayloadConfig::new(500 * 1024 * 1024));

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

        assert!(dockerfile.contains("FROM node:"), "Should have frontend build stage");
        assert!(dockerfile.contains("FROM rust:"), "Should have Rust build stage");
        assert!(dockerfile.contains("FROM debian:"), "Should have runtime stage");
        assert!(dockerfile.contains("cargo build --release"), "Should build in release mode");
        assert!(dockerfile.contains("frontend/dist"), "Should copy frontend build output");
        assert!(dockerfile.contains("EXPOSE 8080"), "Should expose port 8080");
        assert!(dockerfile.contains("migrations/"), "Should include migrations");
    }

    #[test]
    fn test_production_compose_has_required_services() {
        let compose = std::fs::read_to_string("deploy/docker-compose.prod.yml")
            .expect("deploy/docker-compose.prod.yml should exist");

        assert!(compose.contains("nginx:"), "Should have nginx service");
        assert!(compose.contains("app:"), "Should have app service");
        assert!(compose.contains("postgres:"), "Should have postgres service");
        assert!(compose.contains("certbot:"), "Should have certbot service");
        assert!(compose.contains("init-certs:"), "Should have init-certs service");
        assert!(compose.contains("healthcheck:"), "Should have health checks");
        assert!(compose.contains("nginx-prod.conf.template"), "Should use nginx template");
    }

    #[test]
    fn test_production_nginx_has_required_directives() {
        let nginx = std::fs::read_to_string("deploy/docker/nginx-prod.conf.template")
            .expect("deploy/docker/nginx-prod.conf.template should exist");

        assert!(nginx.contains("least_conn"), "Should use least_conn load balancing");
        assert!(nginx.contains("proxy_pass"), "Should have proxy_pass");
        assert!(nginx.contains("client_max_body_size"), "Should set client_max_body_size");
        assert!(nginx.contains("proxy_read_timeout"), "Should set proxy_read_timeout");
        assert!(nginx.contains("/health"), "Should have health check location");
        assert!(nginx.contains("ssl_certificate"), "Should have TLS configuration");
        assert!(nginx.contains("${DOMAIN}"), "Should use DOMAIN template variable");
    }

    #[test]
    fn test_ci_workflow_exists() {
        let ci = std::fs::read_to_string(".github/workflows/ci.yml")
            .expect("CI workflow should exist");

        assert!(ci.contains("cargo clippy"), "Should run clippy");
        assert!(ci.contains("cargo test"), "Should run backend tests");
        assert!(ci.contains("npm run lint"), "Should run frontend lint");
        assert!(ci.contains("npm test"), "Should run frontend tests");
        assert!(ci.contains("npm run build"), "Should build frontend");
    }

    #[test]
    fn test_build_push_workflow_exists() {
        let workflow = std::fs::read_to_string(".github/workflows/build-push.yml")
            .expect("Build-push workflow should exist");

        assert!(workflow.contains("ghcr.io"), "Should use GHCR");
        assert!(workflow.contains("docker/build-push-action"), "Should use build-push action");
        assert!(workflow.contains("GITHUB_TOKEN"), "Should use GITHUB_TOKEN for auth");
        assert!(workflow.contains("ci.yml"), "Should depend on CI workflow");
    }

    #[test]
    fn test_deploy_workflows_exist() {
        let hetzner = std::fs::read_to_string(".github/workflows/deploy-hetzner.yml")
            .expect("Hetzner deploy workflow should exist");
        let windows = std::fs::read_to_string(".github/workflows/deploy-windows.yml")
            .expect("Windows deploy workflow should exist");

        assert!(hetzner.contains("deploy.sh"), "Hetzner should run deploy script");
        assert!(windows.contains("deploy-windows.sh"), "Windows should run deploy-windows script");
    }

    #[test]
    fn test_deploy_script_has_required_features() {
        let script = std::fs::read_to_string("deploy/scripts/deploy.sh")
            .expect("deploy/scripts/deploy.sh should exist");

        assert!(script.contains("--profile"), "Should support --profile flag");
        assert!(script.contains("--image-tag"), "Should support --image-tag flag");
        assert!(script.contains("docker compose"), "Should use docker compose");
        assert!(script.contains("health"), "Should check health after deploy");
        assert!(script.contains("Rollback"), "Should support rollback on failure");
        assert!(script.contains("backup.sh"), "Should run pre-deploy backup");
    }

    #[test]
    fn test_backup_restore_scripts_exist() {
        let backup = std::fs::read_to_string("deploy/scripts/backup.sh")
            .expect("deploy/scripts/backup.sh should exist");
        let restore = std::fs::read_to_string("deploy/scripts/restore.sh")
            .expect("deploy/scripts/restore.sh should exist");
        let cron = std::fs::read_to_string("deploy/scripts/backup-cron.sh")
            .expect("deploy/scripts/backup-cron.sh should exist");

        assert!(backup.contains("pg_dump"), "Backup should use pg_dump");
        assert!(backup.contains("tar.gz"), "Backup should compress to tar.gz");
        assert!(restore.contains("pg_restore"), "Restore should use pg_restore");
        assert!(cron.contains("backup.sh"), "Cron wrapper should call backup.sh");
        assert!(cron.contains("BACKUP_RETENTION_DAYS"), "Cron should support retention");
    }

    #[test]
    fn test_terraform_cloud_init_exists() {
        let cloud_init = std::fs::read_to_string("terraform/cloud-init.yml")
            .expect("terraform/cloud-init.yml should exist");

        assert!(cloud_init.contains("#cloud-config"), "Should be valid cloud-config");
        assert!(cloud_init.contains("docker"), "Should install Docker");
        assert!(cloud_init.contains("deploy"), "Should create deploy user");
        assert!(cloud_init.contains("backup"), "Should set up backup cron");
        assert!(cloud_init.contains("PasswordAuthentication no"), "Should harden SSH");
    }

    #[test]
    fn test_scale_profile_compose_files_exist() {
        let small = std::fs::read_to_string("deploy/docker-compose.small.yml")
            .expect("Small profile should exist");
        let medium = std::fs::read_to_string("deploy/docker-compose.medium.yml")
            .expect("Medium profile should exist");
        let large = std::fs::read_to_string("deploy/docker-compose.large.yml")
            .expect("Large profile should exist");

        assert!(small.contains("app:"), "Small should configure app");
        assert!(medium.contains("postgres-worker-1:"), "Medium should have worker 1");
        assert!(medium.contains("postgres-worker-2:"), "Medium should have worker 2");
        assert!(large.contains("postgres-worker-3:"), "Large should have worker 3");
        assert!(large.contains("postgres-worker-4:"), "Large should have worker 4");
    }
}
