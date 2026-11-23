use serde::Deserialize;
use std::env;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub listen_address: String,
    pub cams_dir: String,
    pub rsa_key_path: String,
    pub cache_dir: String,
    pub max_disk_cache_mb: u64,
    pub max_concurrent_playbacks: u32,
    pub max_concurrent_access_code_playbacks: u32,
    pub database: DatabaseConfig,
}

impl Config {
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Self {
        let mut cfg = if let Ok(content) = fs::read_to_string(&path) {
            serde_json::from_str::<Config>(&content).unwrap_or_else(|e| {
                eprintln!("[Config] Failed to parse config JSON ({}), using defaults", e);
                Self::default()
            })
        } else {
            Self::default()
        };

        // Environment variable overrides
        if let Ok(v) = env::var("LISTEN_ADDR") { cfg.listen_address = v; }
        if let Ok(v) = env::var("CAMS_DIR") { cfg.cams_dir = v; }
        if let Ok(v) = env::var("RSA_KEY_PATH") { cfg.rsa_key_path = v; }
        if let Ok(v) = env::var("CACHE_DIR") { cfg.cache_dir = v; }
        if let Ok(v) = env::var("MAX_DISK_CACHE_MB") {
            if let Ok(n) = v.parse() { cfg.max_disk_cache_mb = n; }
        }
        if let Ok(v) = env::var("MAX_CONCURRENT_PLAYBACKS") {
            if let Ok(n) = v.parse() { cfg.max_concurrent_playbacks = n; }
        }
        if let Ok(v) = env::var("MAX_CONCURRENT_ACCESS_CODE_PLAYBACKS") {
            if let Ok(n) = v.parse() { cfg.max_concurrent_access_code_playbacks = n; }
        }
        if let Ok(v) = env::var("DB_HOST") { cfg.database.host = v; }
        if let Ok(v) = env::var("DB_PORT") {
            if let Ok(p) = v.parse() { cfg.database.port = p; }
        }
        if let Ok(v) = env::var("DB_USER") { cfg.database.user = v; }
        if let Ok(v) = env::var("DB_PASS") { cfg.database.password = v; }
        if let Ok(v) = env::var("DB_NAME") { cfg.database.name = v; }

        cfg
    }

    pub fn default() -> Self {
        Self {
            listen_address: "0.0.0.0:7173".to_string(),
            cams_dir: "cams-ready/".to_string(),
            rsa_key_path: "key.pem".to_string(),
            cache_dir: "cam-cache/".to_string(),
            max_disk_cache_mb: 5120,
            max_concurrent_playbacks: 20,
            max_concurrent_access_code_playbacks: 30,
            database: DatabaseConfig {
                host: "127.0.0.1".to_string(),
                port: 3306,
                user: "forgottenserver".to_string(),
                password: String::new(),
                name: "forgottenserver".to_string(),
            },
        }
    }

    pub fn max_disk_cache_bytes(&self) -> u64 {
        self.max_disk_cache_mb.saturating_mul(1024 * 1024)
    }

    pub fn database_url(&self) -> String {
        format!(
            "mysql://{}:{}@{}:{}/{}",
            self.database.user,
            self.database.password,
            self.database.host,
            self.database.port,
            self.database.name
        )
    }
}
