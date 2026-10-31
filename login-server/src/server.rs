// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

use std::net::{IpAddr, SocketAddr};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinSet;

use crate::cache::{AccountCache, CharListCache, Stats};
use crate::config::Config;
use crate::crypto::{RoundKeys, RsaKey};
use crate::db::Database;
use crate::protocol::{self, ConnMeta, Phase1, ResponseParts, MAX_BODY};

const READ_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const CHARLIST_TIMEOUT: Duration = Duration::from_secs(5);

struct LoginBytes {
    body: Vec<u8>,
    ip: u32,
    otc: bool,
    ha: bool,
}

enum JobKind {
    Phase1(LoginBytes),
    SealList {
        parts: ResponseParts,
        names: Arc<[String]>,
    },
    SealBody {
        body: Vec<u8>,
        keys: RoundKeys,
    },
}

enum JobOut {
    Phase(Phase1),
    Packet(Vec<u8>),
}

struct Job {
    kind: JobKind,
    reply: oneshot::Sender<JobOut>,
}

pub struct WorkerState {
    pub cfg: Arc<Config>,
    pub rsa: Arc<RsaKey>,
    pub accounts: Arc<AccountCache>,
    pub characters: Arc<CharListCache>,
}

pub struct CryptoPool {
    tx: SyncSender<Job>,
}

impl CryptoPool {
    pub fn spawn(threads: usize, state: WorkerState) -> Self {
        let (tx, rx) = sync_channel(state.cfg.pending_login_limit());
        let rx = Arc::new(std::sync::Mutex::new(rx));
        for index in 0..threads.max(1) {
            let rx = Arc::clone(&rx);
            let state = WorkerState {
                cfg: Arc::clone(&state.cfg),
                rsa: Arc::clone(&state.rsa),
                accounts: Arc::clone(&state.accounts),
                characters: Arc::clone(&state.characters),
            };
            std::thread::Builder::new()
                .name(format!("crypto-{index}"))
                .spawn(move || worker_loop(rx, state))
                .expect("spawn crypto thread");
        }
        Self { tx }
    }

    fn try_submit(&self, kind: JobKind) -> Result<oneshot::Receiver<JobOut>, ()> {
        let (reply, rx) = oneshot::channel();
        self.tx.try_send(Job { kind, reply }).map_err(|_| ())?;
        Ok(rx)
    }
}

fn worker_loop(rx: Arc<std::sync::Mutex<Receiver<Job>>>, state: WorkerState) {
    loop {
        let job = {
            let guard = rx.lock().unwrap_or_else(|err| err.into_inner());
            guard.recv()
        };
        let Ok(job) = job else {
            break;
        };
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| process(&state, job.kind)));
        match result {
            Ok(out) => {
                let _ = job.reply.send(out);
            }
            Err(_) => tracing::error!("crypto worker panicked"),
        }
    }
}

fn process(state: &WorkerState, kind: JobKind) -> JobOut {
    match kind {
        JobKind::Phase1(login) => {
            let snap = state.accounts.load_full();
            let phase = protocol::authenticate(
                &login.body,
                ConnMeta {
                    ip: login.ip,
                    otc_proxy: login.otc,
                    ha_proxy: login.ha,
                },
                &state.cfg,
                &state.rsa,
                &snap,
            );
            match phase {
                Phase1::NeedList { account_id, parts } => {
                    if let Some(names) = state.characters.fresh(account_id) {
                        let snap = state.accounts.load_full();
                        let packet = protocol::seal_character_list(&state.cfg, &snap, &parts, &names);
                        JobOut::Phase(Phase1::Accept(packet))
                    } else {
                        JobOut::Phase(Phase1::NeedList { account_id, parts })
                    }
                }
                other => JobOut::Phase(other),
            }
        }
        JobKind::SealList { parts, names } => {
            let snap = state.accounts.load_full();
            JobOut::Packet(protocol::seal_character_list(&state.cfg, &snap, &parts, &names))
        }
        JobKind::SealBody { body, keys } => JobOut::Packet(protocol::seal_xtea(&body, &keys)),
    }
}

pub struct AppState {
    pub cfg: Arc<Config>,
    pub characters: Arc<CharListCache>,
    pub db: Database,
    pub stats: Arc<Stats>,
    pub crypto: CryptoPool,
}

pub async fn serve(state: Arc<AppState>, mut shutdown: watch::Receiver<bool>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(state.cfg.bind_addr()).await?;
    tracing::info!(addr = %state.cfg.bind_addr(), "login server listening");
    let mut conns = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let (socket, peer) = match accepted {
                    Ok(pair) => pair,
                    Err(err) => {
                        tracing::error!(error = %err, "accept failed");
                        break;
                    }
                };
                let state = Arc::clone(&state);
                conns.spawn(async move {
                    if let Err(err) = handle_conn(socket, peer, state).await {
                        tracing::debug!(error = %err, "connection closed");
                    }
                });
            }
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        while conns.join_next().await.is_some() {}
    })
    .await;
    Ok(())
}

async fn handle_conn(mut socket: TcpStream, peer: SocketAddr, state: Arc<AppState>) -> anyhow::Result<()> {
    socket.set_nodelay(true)?;
    let Some(request) = read_request(&mut socket, &state.cfg).await? else {
        return Ok(());
    };
    let ip = request.proxy_ip.unwrap_or_else(|| peer_ip(peer));
    let rx = match state.crypto.try_submit(JobKind::Phase1(LoginBytes {
        body: request.body,
        ip,
        otc: request.otc,
        ha: request.ha,
    })) {
        Ok(rx) => rx,
        Err(()) => {
            state.stats.queue_drops.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
    };
    let Ok(JobOut::Phase(phase)) = rx.await else {
        return Ok(());
    };
    match phase {
        Phase1::Silence => {
            state.stats.logins_fail.fetch_add(1, Ordering::Relaxed);
        }
        Phase1::Reject(packet) => {
            let _ = write_packet(&mut socket, &packet).await;
            state.stats.logins_fail.fetch_add(1, Ordering::Relaxed);
        }
        Phase1::Accept(packet) => {
            write_packet(&mut socket, &packet).await?;
            state.stats.logins_ok.fetch_add(1, Ordering::Relaxed);
        }
        Phase1::NeedList { account_id, parts } => {
            let names = load_characters(&state, account_id).await;
            match names {
                Ok(names) => {
                    let rx = match state.crypto.try_submit(JobKind::SealList { parts, names }) {
                        Ok(rx) => rx,
                        Err(()) => {
                            state.stats.queue_drops.fetch_add(1, Ordering::Relaxed);
                            return Ok(());
                        }
                    };
                    if let Ok(JobOut::Packet(packet)) = rx.await {
                        write_packet(&mut socket, &packet).await?;
                        state.stats.logins_ok.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(()) => {
                    let body = protocol::error_body(parts.version, "Failed to load character list.");
                    let keys = parts.round_keys;
                    if let Ok(rx) = state.crypto.try_submit(JobKind::SealBody { body, keys }) {
                        if let Ok(JobOut::Packet(packet)) = rx.await {
                            let _ = write_packet(&mut socket, &packet).await;
                        }
                    }
                    state.stats.logins_fail.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
    let _ = socket.shutdown().await;
    Ok(())
}

async fn load_characters(state: &AppState, account_id: u32) -> Result<Arc<[String]>, ()> {
    let db = state.db.clone();
    let stats = Arc::clone(&state.stats);
    state
        .characters
        .get_or_load(account_id, move || async move {
            stats.charlist_queries.fetch_add(1, Ordering::Relaxed);
            match tokio::time::timeout(CHARLIST_TIMEOUT, db.character_names(account_id)).await {
                Ok(Ok(names)) => Ok(names),
                Ok(Err(err)) => Err(err.to_string()),
                Err(_) => Err("character list query timed out".to_owned()),
            }
        })
        .await
}

struct RawRequest {
    body: Vec<u8>,
    proxy_ip: Option<u32>,
    otc: bool,
    ha: bool,
}

async fn read_request(socket: &mut TcpStream, cfg: &Config) -> anyhow::Result<Option<RawRequest>> {
    let result = tokio::time::timeout(READ_TIMEOUT, read_request_inner(socket, cfg)).await;
    match result {
        Ok(inner) => inner,
        Err(_) => Ok(None),
    }
}

async fn read_request_inner(socket: &mut TcpStream, cfg: &Config) -> anyhow::Result<Option<RawRequest>> {
    let mut header = [0u8; 2];
    if socket.read_exact(&mut header).await.is_err() {
        return Ok(None);
    }
    let mut size = u16::from_le_bytes(header);
    let mut proxy_ip = None;
    let mut otc = false;
    let mut ha = false;
    if cfg.allow_otc_proxy && size == 0xFFFE {
        let mut ip = [0u8; 4];
        socket.read_exact(&mut ip).await?;
        proxy_ip = Some(u32::from_le_bytes(ip));
        otc = true;
        socket.read_exact(&mut header).await?;
        size = u16::from_le_bytes(header);
    } else if cfg.allow_haproxy && size == 0x0A0D {
        let mut rest = [0u8; 26];
        socket.read_exact(&mut rest).await?;
        proxy_ip = Some(u32::from_le_bytes(rest[14..18].try_into().unwrap()));
        ha = true;
        socket.read_exact(&mut header).await?;
        size = u16::from_le_bytes(header);
    }
    if size == 0 || usize::from(size) > MAX_BODY {
        return Ok(None);
    }
    let mut body = vec![0u8; size as usize];
    socket.read_exact(&mut body).await?;
    Ok(Some(RawRequest {
        body,
        proxy_ip,
        otc,
        ha,
    }))
}

async fn write_packet(socket: &mut TcpStream, packet: &[u8]) -> anyhow::Result<()> {
    tokio::time::timeout(WRITE_TIMEOUT, socket.write_all(packet))
        .await
        .map_err(|_| anyhow::anyhow!("write timed out"))??;
    Ok(())
}

fn peer_ip(peer: SocketAddr) -> u32 {
    match peer.ip() {
        IpAddr::V4(ip) => protocol::tfs_ipv4(ip.octets()),
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(|ip| protocol::tfs_ipv4(ip.octets()))
            .unwrap_or(0),
    }
}
