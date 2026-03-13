use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default = "default_server")]
    pub server: ServerConfig,
    #[serde(default = "default_database")]
    pub database: DatabaseConfig,
    #[serde(default = "default_storage")]
    pub storage: StorageConfig,
    #[serde(default = "default_sync")]
    pub sync: SyncConfig,
    #[serde(default = "default_auth")]
    pub auth: AuthConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_database_url")]
    pub url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageConfig {
    #[serde(default = "default_local_temp_path")]
    pub local_temp_path: String,
    #[serde(default = "default_hmac_secret", skip_serializing)]
    pub hmac_secret: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    #[serde(default = "default_jwt_secret", skip_serializing)]
    pub jwt_secret: String,
    #[serde(default = "default_access_token_ttl_secs")]
    pub access_token_ttl_secs: u64,
    #[serde(default = "default_refresh_token_ttl_secs")]
    pub refresh_token_ttl_secs: u64,
    #[serde(default = "default_admin_username")]
    pub default_admin_username: String,
    #[serde(default = "default_admin_password", skip_serializing)]
    pub default_admin_password: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SyncConfig {
    #[serde(default = "default_num_workers")]
    pub num_workers: usize,
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_tier_scan_interval_secs")]
    pub tier_scan_interval_secs: u64,
}

fn default_server() -> ServerConfig {
    ServerConfig {
        host: default_host(),
        port: default_port(),
    }
}

fn default_database() -> DatabaseConfig {
    DatabaseConfig {
        url: default_database_url(),
        max_connections: default_max_connections(),
    }
}

fn default_storage() -> StorageConfig {
    StorageConfig {
        local_temp_path: default_local_temp_path(),
        hmac_secret: default_hmac_secret(),
    }
}

fn default_sync() -> SyncConfig {
    SyncConfig {
        num_workers: default_num_workers(),
        max_retries: default_max_retries(),
        poll_interval_secs: default_poll_interval_secs(),
        tier_scan_interval_secs: default_tier_scan_interval_secs(),
    }
}

fn default_auth() -> AuthConfig {
    AuthConfig {
        jwt_secret: default_jwt_secret(),
        access_token_ttl_secs: default_access_token_ttl_secs(),
        refresh_token_ttl_secs: default_refresh_token_ttl_secs(),
        default_admin_username: default_admin_username(),
        default_admin_password: default_admin_password(),
    }
}

fn default_jwt_secret() -> String {
    "change-me-jwt-secret-in-production".to_string()
}

fn default_access_token_ttl_secs() -> u64 {
    900
}

fn default_refresh_token_ttl_secs() -> u64 {
    604800
}

fn default_admin_username() -> String {
    "admin".to_string()
}

fn default_admin_password() -> String {
    "Innovare2026!".to_string()
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_database_url() -> String {
    "postgres://localhost:5432/innovare_storage".to_string()
}

fn default_max_connections() -> u32 {
    10
}

fn default_local_temp_path() -> String {
    "./data/temp".to_string()
}

fn default_hmac_secret() -> String {
    "change-me-in-production".to_string()
}

fn default_num_workers() -> usize {
    4
}

fn default_max_retries() -> i32 {
    5
}

fn default_poll_interval_secs() -> u64 {
    5
}

fn default_tier_scan_interval_secs() -> u64 {
    300
}

impl AppConfig {
    /// Load configuration from TOML file with environment variable overrides.
    ///
    /// Looks for config files in order:
    /// 1. `config/default.toml`
    /// 2. Environment variables with prefix `APP_` and separator `__`
    ///    (e.g., APP_SERVER__PORT=9090)
    pub fn load() -> Result<Self, config::ConfigError> {
        Self::load_from("config/default")
    }

    /// Load configuration from a specific file path (without extension) with env overrides.
    pub fn load_from(path: &str) -> Result<Self, config::ConfigError> {
        let builder = config::Config::builder()
            .add_source(
                config::File::with_name(path).required(false),
            )
            .add_source(
                config::Environment::with_prefix("APP")
                    .prefix_separator("_")
                    .separator("__"),
            );

        builder.build()?.try_deserialize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;

    // Mutex to serialize tests that touch environment variables
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_default_config() {
        let _lock = ENV_MUTEX.lock().unwrap();
        // Ensure no leftover env vars from other tests
        std::env::remove_var("APP_SERVER__PORT");
        std::env::remove_var("APP_SERVER__HOST");

        let cfg = AppConfig::load_from("nonexistent_path").unwrap();
        assert_eq!(cfg.server.host, "0.0.0.0");
        assert_eq!(cfg.server.port, 8080);
        assert_eq!(cfg.database.max_connections, 10);
        assert_eq!(cfg.sync.num_workers, 4);
        assert_eq!(cfg.sync.max_retries, 5);
        assert_eq!(cfg.sync.poll_interval_secs, 5);
        assert_eq!(cfg.sync.tier_scan_interval_secs, 300);
        assert_eq!(cfg.storage.local_temp_path, "./data/temp");
    }

    #[test]
    fn test_load_from_toml_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("test_config.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        writeln!(
            f,
            r#"
[server]
host = "127.0.0.1"
port = 3000

[database]
url = "postgres://user:pass@db:5432/mydb"
max_connections = 20

[storage]
local_temp_path = "/tmp/storage"
hmac_secret = "super-secret"

[sync]
num_workers = 8
"#
        )
        .unwrap();

        let path_no_ext = config_path.to_str().unwrap().trim_end_matches(".toml");
        let cfg = AppConfig::load_from(path_no_ext).unwrap();
        assert_eq!(cfg.server.host, "127.0.0.1");
        assert_eq!(cfg.server.port, 3000);
        assert_eq!(cfg.database.url, "postgres://user:pass@db:5432/mydb");
        assert_eq!(cfg.database.max_connections, 20);
        assert_eq!(cfg.storage.local_temp_path, "/tmp/storage");
        assert_eq!(cfg.storage.hmac_secret, "super-secret");
        assert_eq!(cfg.sync.num_workers, 8);
    }

    #[test]
    fn test_env_var_overrides() {
        let _lock = ENV_MUTEX.lock().unwrap();
        // Set env vars temporarily
        std::env::set_var("APP_SERVER__PORT", "9090");
        std::env::set_var("APP_SERVER__HOST", "localhost");

        let cfg = AppConfig::load_from("nonexistent_path").unwrap();
        assert_eq!(cfg.server.port, 9090);
        assert_eq!(cfg.server.host, "localhost");

        // Clean up
        std::env::remove_var("APP_SERVER__PORT");
        std::env::remove_var("APP_SERVER__HOST");
    }
}
