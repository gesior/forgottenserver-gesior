// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

mod cache;
mod config;
mod crypto;
mod db;
mod protocol;
mod server;

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing_subscriber::EnvFilter;

use crate::cache::{AccountCache, CharListCache, Stats};
use crate::config::Config;
use crate::crypto::RsaKey;
use crate::db::{Database, SyncCursor};
use crate::server::{AppState, CryptoPool, WorkerState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let path = std::env::args().nth(1).unwrap_or_else(|| "config.toml".to_owned());
    let cfg = Arc::new(Config::load(std::path::Path::new(&path))?);
    let rsa = Arc::new(RsaKey::load(std::path::Path::new(&cfg.rsa_key_file))?);
    let database = Database::connect(&cfg).await?;
    database.ensure_schema().await?;

    let accounts = Arc::new(AccountCache::new());
    let characters = Arc::new(CharListCache::new(Duration::from_secs(
        cfg.cache.character_list_ttl_seconds,
    )));
    let stats = Arc::new(Stats::default());
    let mut cursor = SyncCursor::default();
    let updated = database.sync_once(&mut cursor, &accounts, &cfg, i64::MAX).await?;
    if cursor.unix > 0 {
        cursor.unix -= 1;
        cursor.id = 0;
    }
    tracing::info!(
        updated,
        accounts = accounts.len(),
        bans = accounts.load_full().ip_bans.len(),
        "login cache ready"
    );

    let threads = cfg.crypto_thread_count();
    let crypto = CryptoPool::spawn(
        threads,
        WorkerState {
            cfg: Arc::clone(&cfg),
            rsa,
            accounts: Arc::clone(&accounts),
            characters: Arc::clone(&characters),
        },
    );
    tracing::info!(threads, "crypto workers started");

    let state = Arc::new(AppState {
        cfg: Arc::clone(&cfg),
        characters: Arc::clone(&characters),
        db: database.clone(),
        stats: Arc::clone(&stats),
        crypto,
    });
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let server = tokio::spawn(server::serve(state, shutdown_rx.clone()));
    let poller = tokio::spawn(db::run_poller(
        database,
        accounts,
        characters,
        cfg,
        cursor,
        stats,
        shutdown_rx,
    ));

    tokio::signal::ctrl_c().await?;
    tracing::info!("shutting down");
    let _ = shutdown_tx.send(true);
    let _ = server.await;
    let _ = poller.await;
    Ok(())
}
