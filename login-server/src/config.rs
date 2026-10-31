// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

use std::net::SocketAddr;
use std::path::Path;

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub bind: String,
    pub rsa_key_file: String,
    #[serde(default)]
    pub crypto_threads: usize,
    #[serde(default = "default_pending")]
    pub max_pending_logins: usize,
    #[serde(default)]
    pub allow_otc_proxy: bool,
    #[serde(default)]
    pub allow_haproxy: bool,
    pub server: ServerConfig,
    pub mysql: MysqlConfig,
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub ip: String,
    #[serde(default)]
    pub status_ip: String,
    pub game_port: u16,
    #[serde(default)]
    pub motd: String,
    #[serde(default)]
    pub free_premium: bool,
    #[serde(default)]
    pub status: ServerStatus,
    #[serde(default = "default_version_min")]
    pub client_version_min: u16,
    #[serde(default = "default_version_max")]
    pub client_version_max: u16,
    #[serde(default = "default_version_string")]
    pub client_version_string: String,
    #[serde(default)]
    pub show_online_status: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    #[default]
    Online,
    Starting,
    Maintenance,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MysqlConfig {
    pub host: String,
    #[serde(default = "default_mysql_port")]
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub password: String,
    pub database: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_poll")]
    pub account_poll_interval_seconds: u64,
    #[serde(default = "default_reconcile")]
    pub account_reconcile_interval_seconds: u64,
    #[serde(default = "default_ttl")]
    pub character_list_ttl_seconds: u64,
}

fn default_pending() -> usize {
    4096
}

fn default_version_min() -> u16 {
    1097
}

fn default_version_max() -> u16 {
    1098
}

fn default_version_string() -> String {
    "10.98".to_owned()
}

fn default_mysql_port() -> u16 {
    3306
}

fn default_poll() -> u64 {
    1
}

fn default_reconcile() -> u64 {
    60
}

fn default_ttl() -> u64 {
    30
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        Self::from_toml(&text)
    }

    pub fn from_toml(text: &str) -> anyhow::Result<Self> {
        let cfg: Self = toml::from_str(text).context("failed to parse config.toml")?;
        cfg.bind
            .parse::<SocketAddr>()
            .with_context(|| format!("invalid bind address {}", cfg.bind))?;
        if cfg.server.client_version_min > cfg.server.client_version_max {
            anyhow::bail!("client_version_min is greater than client_version_max");
        }
        if cfg.server.game_port == 0 {
            anyhow::bail!("game_port must not be 0");
        }
        if cfg.cache.account_poll_interval_seconds == 0 {
            anyhow::bail!("account_poll_interval_seconds must be at least 1");
        }
        if cfg.cache.account_reconcile_interval_seconds == 0 {
            anyhow::bail!("account_reconcile_interval_seconds must be at least 1");
        }
        Ok(cfg)
    }

    pub fn bind_addr(&self) -> SocketAddr {
        self.bind.parse().expect("bind address was validated")
    }

    pub fn crypto_thread_count(&self) -> usize {
        if self.crypto_threads == 0 {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
        } else {
            self.crypto_threads
        }
    }

    pub fn pending_login_limit(&self) -> usize {
        if self.max_pending_logins == 0 {
            default_pending()
        } else {
            self.max_pending_logins
        }
    }
}
