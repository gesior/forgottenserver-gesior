// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use futures::future::{BoxFuture, Shared};
use futures::FutureExt;
use tokio::sync::Mutex;

pub type SharedLoad = Shared<BoxFuture<'static, Result<Arc<[String]>, Arc<str>>>>;

#[derive(Debug, Default)]
pub struct Stats {
    pub logins_ok: AtomicU64,
    pub logins_fail: AtomicU64,
    pub charlist_queries: AtomicU64,
    pub queue_drops: AtomicU64,
}

impl Stats {
    pub fn log_and_reset(&self) {
        tracing::info!(
            ok = self.logins_ok.swap(0, Ordering::Relaxed),
            rejected = self.logins_fail.swap(0, Ordering::Relaxed),
            charlist_queries = self.charlist_queries.swap(0, Ordering::Relaxed),
            queue_drops = self.queue_drops.swap(0, Ordering::Relaxed),
            "login stats"
        );
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// ASCII case-fold, matching the usual `utf8` account-name collation for OTS names.
pub fn name_key(name: &[u8]) -> Vec<u8> {
    name.iter()
        .map(|byte| byte.to_ascii_lowercase())
        .collect()
}

#[derive(Debug, Clone)]
pub struct AccountRecord {
    pub name: String,
    pub password_sha1: String,
    pub secret: Vec<u8>,
    pub premium_ends_at: u32,
}

#[derive(Debug, Clone)]
pub struct AccountUpdate {
    pub id: u32,
    pub name: String,
    pub password_sha1: String,
    pub secret: Vec<u8>,
    pub premium_ends_at: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpBan {
    pub expires_at: i64,
    pub reason: String,
    pub banned_by: String,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub by_id: HashMap<u32, Arc<AccountRecord>>,
    pub by_name: HashMap<Vec<u8>, u32>,
    pub ip_bans: HashMap<u32, IpBan>,
    pub motd_num: u32,
    pub online_names: HashSet<String>,
}

impl Snapshot {
    pub fn upsert(&mut self, update: AccountUpdate) {
        let new_key = name_key(update.name.as_bytes());
        if let Some(previous) = self.by_id.get(&update.id) {
            let old_key = name_key(previous.name.as_bytes());
            if old_key != new_key && self.by_name.get(&old_key) == Some(&update.id) {
                self.by_name.remove(&old_key);
            }
        }
        if let Some(other) = self.by_name.get(&new_key) {
            if *other != update.id {
                tracing::warn!(
                    account_id = update.id,
                    other_id = *other,
                    "account name lookup replaced an existing key"
                );
            }
        }
        self.by_name.insert(new_key, update.id);
        self.by_id.insert(
            update.id,
            Arc::new(AccountRecord {
                name: update.name,
                password_sha1: update.password_sha1.to_ascii_lowercase(),
                secret: update.secret,
                premium_ends_at: update.premium_ends_at,
            }),
        );
    }
}

pub struct AccountCache {
    snapshot: ArcSwap<Snapshot>,
}

impl AccountCache {
    pub fn new() -> Self {
        Self {
            snapshot: ArcSwap::from_pointee(Snapshot::default()),
        }
    }

    pub fn load_full(&self) -> Arc<Snapshot> {
        self.snapshot.load_full()
    }

    pub fn len(&self) -> usize {
        self.snapshot.load().by_id.len()
    }

    pub fn publish(
        &self,
        account_changes: Vec<AccountUpdate>,
        bans: HashMap<u32, IpBan>,
        motd_num: u32,
        online: Option<HashSet<String>>,
    ) -> bool {
        let current = self.snapshot.load_full();
        let accounts_changed = !account_changes.is_empty();
        let bans_changed = bans != current.ip_bans;
        let motd_changed = motd_num != current.motd_num;
        let online_changed = online.as_ref().is_some_and(|names| names != &current.online_names);
        if !accounts_changed && !bans_changed && !motd_changed && !online_changed {
            return false;
        }
        let mut next = Snapshot {
            by_id: current.by_id.clone(),
            by_name: current.by_name.clone(),
            ip_bans: bans,
            motd_num,
            online_names: online.unwrap_or_else(|| current.online_names.clone()),
        };
        for update in account_changes {
            next.upsert(update);
        }
        self.snapshot.store(Arc::new(next));
        true
    }

    pub fn remove_ids(&self, ids: &[u32]) -> usize {
        if ids.is_empty() {
            return 0;
        }
        let current = self.snapshot.load_full();
        let mut next = (*current).clone();
        let mut removed = 0;
        for id in ids {
            if let Some(record) = next.by_id.remove(id) {
                let key = name_key(record.name.as_bytes());
                if next.by_name.get(&key) == Some(id) {
                    next.by_name.remove(&key);
                }
                removed += 1;
            }
        }
        if removed > 0 {
            self.snapshot.store(Arc::new(next));
        }
        removed
    }
}

struct CharEntry {
    names: Arc<[String]>,
    fetched_at: Instant,
}

pub struct CharListCache {
    ttl: Duration,
    entries: Arc<DashMap<u32, CharEntry>>,
    inflight: Arc<Mutex<HashMap<u32, SharedLoad>>>,
}

impl CharListCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Arc::new(DashMap::new()),
            inflight: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn fresh(&self, account_id: u32) -> Option<Arc<[String]>> {
        let entry = self.entries.get(&account_id)?;
        if entry.fetched_at.elapsed() < self.ttl {
            Some(Arc::clone(&entry.names))
        } else {
            None
        }
    }

    pub fn cached(&self, account_id: u32) -> Option<Arc<[String]>> {
        self.entries.get(&account_id).map(|entry| Arc::clone(&entry.names))
    }

    pub fn invalidate(&self, account_id: u32) {
        self.entries.remove(&account_id);
    }

    pub async fn get_or_load<F, Fut>(&self, account_id: u32, load: F) -> Result<Arc<[String]>, ()>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<String>, String>> + Send + 'static,
    {
        if let Some(names) = self.fresh(account_id) {
            return Ok(names);
        }
        let stale = self.cached(account_id);
        let shared = {
            let mut guard = self.inflight.lock().await;
            if let Some(names) = self.fresh(account_id) {
                return Ok(names);
            }
            if let Some(existing) = guard.get(&account_id) {
                existing.clone()
            } else {
                let entries = Arc::clone(&self.entries);
                let inflight = Arc::clone(&self.inflight);
                let load = load();
                let shared = async move {
                    let result = match load.await {
                        Ok(names) => {
                            let names: Arc<[String]> = names.into();
                            entries.insert(
                                account_id,
                                CharEntry {
                                    names: Arc::clone(&names),
                                    fetched_at: Instant::now(),
                                },
                            );
                            Ok(names)
                        }
                        Err(err) => Err(Arc::<str>::from(err)),
                    };
                    inflight.lock().await.remove(&account_id);
                    result
                }
                .boxed()
                .shared();
                guard.insert(account_id, shared.clone());
                shared
            }
        };
        match shared.await {
            Ok(names) => Ok(names),
            Err(_) => stale.or_else(|| self.cached(account_id)).ok_or(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_key_folds_ascii_only() {
        assert_eq!(name_key(b"Admin"), b"admin");
        assert_eq!(name_key(b"aDmin"), b"admin");
    }

    #[test]
    fn upsert_replaces_old_name_key() {
        let mut snapshot = Snapshot::default();
        snapshot.upsert(AccountUpdate {
            id: 1,
            name: "Admin".to_owned(),
            password_sha1: "ABC".to_owned(),
            secret: Vec::new(),
            premium_ends_at: 0,
        });
        snapshot.upsert(AccountUpdate {
            id: 1,
            name: "Other".to_owned(),
            password_sha1: "def".to_owned(),
            secret: Vec::new(),
            premium_ends_at: 5,
        });
        assert!(snapshot.by_name.get(b"admin".as_slice()).is_none());
        assert_eq!(snapshot.by_name.get(b"other".as_slice()), Some(&1));
        assert_eq!(snapshot.by_id[&1].password_sha1, "def");
        assert_eq!(snapshot.by_id[&1].premium_ends_at, 5);
    }
}
