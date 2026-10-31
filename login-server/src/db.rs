// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlRow};
use sqlx::{MySqlPool, Row};
use tokio::sync::watch;
use tokio::time::{interval, MissedTickBehavior};

use crate::cache::{
    unix_now, AccountCache, AccountUpdate, CharListCache, IpBan, Stats,
};
use crate::config::Config;
use crate::crypto;

const PAGE: i64 = 5000;

const ADD_UPDATED_AT: &str = "\
ALTER TABLE `accounts` \
ADD COLUMN `updated_at` TIMESTAMP NOT NULL \
DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP, \
ADD INDEX `accounts_updated_at_idx` (`updated_at`, `id`)";

const ADD_UPDATED_AT_INDEX: &str = "\
ALTER TABLE `accounts` ADD INDEX `accounts_updated_at_idx` (`updated_at`, `id`)";

#[derive(Clone, Copy, Debug)]
pub struct SyncCursor {
    pub unix: i64,
    pub id: u32,
}

impl Default for SyncCursor {
    fn default() -> Self {
        Self { unix: 0, id: 0 }
    }
}

#[derive(Clone)]
pub struct Database {
    pool: MySqlPool,
}

struct AccountRow {
    update: AccountUpdate,
    updated_unix: i64,
}

impl Database {
    pub async fn connect(cfg: &Config) -> anyhow::Result<Self> {
        let opts = MySqlConnectOptions::new()
            .host(&cfg.mysql.host)
            .port(cfg.mysql.port)
            .username(&cfg.mysql.user)
            .password(&cfg.mysql.password)
            .database(&cfg.mysql.database)
            .charset("utf8mb4");
        let pool = MySqlPoolOptions::new()
            .max_connections(16)
            .acquire_timeout(Duration::from_secs(10))
            .after_connect(|conn, _| {
                Box::pin(async move {
                    sqlx::query("SET time_zone = '+00:00'")
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            })
            .connect_with(opts)
            .await?;
        Ok(Self { pool })
    }

    pub async fn ensure_schema(&self) -> anyhow::Result<()> {
        let column: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'accounts' AND COLUMN_NAME = 'updated_at'",
        )
        .fetch_one(&self.pool)
        .await?;
        if column == 0 {
            tracing::info!("adding accounts.updated_at");
            sqlx::query(ADD_UPDATED_AT)
                .execute(&self.pool)
                .await
                .map_err(|err| anyhow::anyhow!("{err}\nSQL: {ADD_UPDATED_AT}"))?;
            return Ok(());
        }
        let index: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'accounts' AND INDEX_NAME = 'accounts_updated_at_idx'",
        )
        .fetch_one(&self.pool)
        .await?;
        if index == 0 {
            tracing::info!("adding accounts_updated_at_idx");
            sqlx::query(ADD_UPDATED_AT_INDEX)
                .execute(&self.pool)
                .await
                .map_err(|err| anyhow::anyhow!("{err}\nSQL: {ADD_UPDATED_AT_INDEX}"))?;
        }
        Ok(())
    }

    pub async fn sync_once(
        &self,
        cursor: &mut SyncCursor,
        accounts: &AccountCache,
        cfg: &Config,
        cutoff: i64,
    ) -> anyhow::Result<usize> {
        let cutoff = cutoff.max(0);
        let mut next = *cursor;
        let mut changes = Vec::new();
        loop {
            let page = self.fetch_account_page(next).await?;
            if page.is_empty() {
                break;
            }
            let mut stop = false;
            for row in page {
                if row.updated_unix > cutoff {
                    stop = true;
                    break;
                }
                next = SyncCursor {
                    unix: row.updated_unix,
                    id: row.update.id,
                };
                changes.push(row.update);
            }
            if stop {
                break;
            }
        }
        let bans = self.load_bans().await?;
        let motd_num = self.load_motd_num().await?;
        let online = if cfg.server.show_online_status {
            Some(self.load_online_names().await?)
        } else {
            None
        };
        let count = changes.len();
        accounts.publish(changes, bans, motd_num, online);
        *cursor = next;
        Ok(count)
    }

    pub async fn reconcile(
        &self,
        accounts: &AccountCache,
        characters: &CharListCache,
    ) -> anyhow::Result<usize> {
        let mut live = HashSet::new();
        let mut last_id = 0u32;
        loop {
            let rows = sqlx::query("SELECT `id` FROM `accounts` WHERE `id` > ? ORDER BY `id` ASC LIMIT 20000")
                .bind(last_id)
                .fetch_all(&self.pool)
                .await?;
            if rows.is_empty() {
                break;
            }
            for row in &rows {
                let id = u32::try_from(row.try_get::<i32, _>("id")?)?;
                live.insert(id);
                last_id = id;
            }
            if rows.len() < 20000 {
                break;
            }
        }
        let current = accounts.load_full();
        let stale: Vec<u32> = current
            .by_id
            .keys()
            .copied()
            .filter(|id| !live.contains(id))
            .collect();
        for id in &stale {
            characters.invalidate(*id);
        }
        Ok(accounts.remove_ids(&stale))
    }

    pub async fn character_names(&self, account_id: u32) -> Result<Vec<String>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT `name` FROM `players` WHERE `account_id` = ? AND `deletion` = 0 ORDER BY `name` ASC",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;
        let mut names = Vec::with_capacity(rows.len());
        for row in rows {
            names.push(row.try_get("name")?);
        }
        Ok(names)
    }

    async fn fetch_account_page(&self, cursor: SyncCursor) -> Result<Vec<AccountRow>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT `id`, `name`, `password`, `secret`, `premium_ends_at`, \
                    UNIX_TIMESTAMP(`updated_at`) AS `updated_unix` \
             FROM `accounts` \
             WHERE `updated_at` > FROM_UNIXTIME(?) \
                OR (`updated_at` = FROM_UNIXTIME(?) AND `id` > ?) \
             ORDER BY `updated_at` ASC, `id` ASC \
             LIMIT ?",
        )
        .bind(cursor.unix)
        .bind(cursor.unix)
        .bind(cursor.id)
        .bind(PAGE)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(account_row(&row)?);
        }
        Ok(out)
    }

    async fn load_bans(&self) -> Result<HashMap<u32, IpBan>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT `ip`, `reason`, `expires_at`, \
                    (SELECT `name` FROM `players` WHERE `id` = `banned_by`) AS `banned_by` \
             FROM `ip_bans`",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut bans = HashMap::with_capacity(rows.len());
        for row in rows {
            let ip = row.try_get::<u32, _>("ip")?;
            let reason: String = row.try_get("reason")?;
            let expires_at = row.try_get::<i64, _>("expires_at")?;
            let banned_by = row.try_get::<Option<String>, _>("banned_by")?.unwrap_or_default();
            bans.insert(
                ip,
                IpBan {
                    expires_at,
                    reason,
                    banned_by,
                },
            );
        }
        Ok(bans)
    }

    async fn load_motd_num(&self) -> Result<u32, sqlx::Error> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT `value` FROM `server_config` WHERE `config` = 'motd_num' LIMIT 1")
                .fetch_optional(&self.pool)
                .await?;
        Ok(value.and_then(|text| text.parse().ok()).unwrap_or(0))
    }

    async fn load_online_names(&self) -> Result<HashSet<String>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT `p`.`name` AS `name` FROM `players_online` AS `o` \
             INNER JOIN `players` AS `p` ON `p`.`id` = `o`.`player_id`",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut names = HashSet::with_capacity(rows.len());
        for row in rows {
            names.insert(row.try_get("name")?);
        }
        Ok(names)
    }
}

fn account_row(row: &MySqlRow) -> Result<AccountRow, sqlx::Error> {
    let id = u32::try_from(row.try_get::<i32, _>("id")?).map_err(|err| sqlx::Error::Decode(err.into()))?;
    let name: String = row.try_get("name")?;
    let password_sha1: String = row.try_get("password")?;
    let secret: Option<String> = row.try_get("secret")?;
    let premium_ends_at = row.try_get::<u32, _>("premium_ends_at")?;
    let updated_unix = row.try_get::<i64, _>("updated_unix")?;
    Ok(AccountRow {
        update: AccountUpdate {
            id,
            name,
            password_sha1,
            secret: secret
                .as_deref()
                .map(crypto::decode_secret)
                .unwrap_or_default(),
            premium_ends_at,
        },
        updated_unix,
    })
}

pub async fn run_poller(
    db: Database,
    accounts: Arc<AccountCache>,
    characters: Arc<CharListCache>,
    cfg: Arc<Config>,
    mut cursor: SyncCursor,
    stats: Arc<Stats>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut poll = interval(Duration::from_secs(cfg.cache.account_poll_interval_seconds));
    poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
    poll.tick().await;
    let mut reconcile = interval(Duration::from_secs(cfg.cache.account_reconcile_interval_seconds));
    reconcile.set_missed_tick_behavior(MissedTickBehavior::Delay);
    reconcile.tick().await;
    let mut stats_tick = interval(Duration::from_secs(60));
    stats_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    stats_tick.tick().await;

    loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            _ = poll.tick() => {
                let cutoff = (unix_now() as i64).saturating_sub(1);
                match db.sync_once(&mut cursor, &accounts, &cfg, cutoff).await {
                    Ok(0) => tracing::debug!("account poll: no account changes"),
                    Ok(updated) => tracing::info!(updated, accounts = accounts.len(), "account snapshot updated"),
                    Err(err) => tracing::error!(error = %err, "account poll failed"),
                }
            }
            _ = reconcile.tick() => {
                match db.reconcile(&accounts, &characters).await {
                    Ok(0) => tracing::debug!("account reconcile: no deletions"),
                    Ok(removed) => tracing::info!(removed, "removed deleted accounts from cache"),
                    Err(err) => tracing::error!(error = %err, "account reconcile failed"),
                }
            }
            _ = stats_tick.tick() => stats.log_and_reset(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migration_runs_when_database_url_is_set() {
        let Ok(url) = std::env::var("LOGIN_SERVER_DATABASE_URL") else {
            return;
        };
        let pool = MySqlPool::connect(&url).await.expect("connect");
        let db = Database { pool };
        db.ensure_schema().await.expect("migrate");
        db.ensure_schema().await.expect("migrate again");
    }
}
