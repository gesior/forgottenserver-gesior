// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use login_server::crypto::{self, RsaKey};
use login_server::protocol::{self, LoginOutcome};
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use sqlx::{MySqlPool, Row};

const THREADS: usize = 12;
const ACCOUNTS: u32 = 50_000;
const MEASURE_SECS: u64 = 10;
const BATCH: u32 = 1000;
const MISSING: u32 = 4096;
const PASSWORD: &str = "benchmark";
const XTEA_KEY: [u32; 4] = [1, 2, 3, 4];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mysql_host = env_or("MYSQL_HOST", "host.docker.internal");
    let mysql_port: u16 = env_or("MYSQL_PORT", "3306").parse()?;
    let mysql_user = env_or("MYSQL_USER", "root");
    let mysql_password = env_or("MYSQL_PASSWORD", "root");
    let mysql_database = env_or("MYSQL_DATABASE", "compilation");
    let rust_addr = env_or("BENCH_RUST", "login:7173");
    let ots_addr = env_or("BENCH_OTS", "server:7171");
    let key_path = env_or("RSA_KEY_FILE", "/key.pem");
    let cycle: u32 = env_or("BENCH_CYCLE", "0").parse()?;
    let cycle = if cycle == 0 { ACCOUNTS } else { cycle.min(ACCOUNTS) };
    let existing_label = if cycle == ACCOUNTS {
        "existing"
    } else {
        "hot"
    };

    let pool = MySqlPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(30))
        .connect_with(
            MySqlConnectOptions::new()
                .host(&mysql_host)
                .port(mysql_port)
                .username(&mysql_user)
                .password(&mysql_password)
                .database(&mysql_database),
        )
        .await?;
    seed(&pool).await?;
    pool.close().await;

    let rsa = RsaKey::load(std::path::Path::new(&key_path))?;
    eprintln!("encrypting login packets on {THREADS} threads ({cycle} existing accounts in the loop)");
    let existing_names: Vec<String> = (0..cycle).map(account_name).collect();
    let missing_names: Vec<String> = (0..MISSING).map(|i| format!("nouser{i:06}")).collect();
    let existing = Arc::new(precompute(&rsa, &existing_names));
    let missing = Arc::new(precompute(&rsa, &missing_names));

    wait_port(&rust_addr)?;
    wait_port(&ots_addr)?;
    wait_until(&rust_addr, &existing[0], LoginOutcome::CharacterList, Duration::from_secs(60))?;

    let mut rows = Vec::new();
    for (label, addr, packets, expect) in [
        (format!("Rust {existing_label}"), rust_addr.as_str(), Arc::clone(&existing), LoginOutcome::CharacterList),
        ("Rust missing".to_owned(), rust_addr.as_str(), Arc::clone(&missing), LoginOutcome::Rejected),
        (format!("OTS {existing_label}"), ots_addr.as_str(), Arc::clone(&existing), LoginOutcome::CharacterList),
        ("OTS missing".to_owned(), ots_addr.as_str(), Arc::clone(&missing), LoginOutcome::Rejected),
    ] {
        if expect == LoginOutcome::CharacterList {
            let warm = warmup(addr, &packets);
            let warm_rate = warm.ok as f64 / warm.elapsed.as_secs_f64();
            eprintln!(
                "warmup {label}: {warm_rate:.1}/s, {} ok, {} connect, {} io, {} other in {:.1}s",
                warm.ok, warm.connect, warm.io, warm.other, warm.elapsed.as_secs_f64()
            );
            if warm.ok == 0 {
                anyhow::bail!("{label} warmup produced no character lists");
            }
        }
        let sample = measure(addr, &packets, expect);
        let rate = sample.ok as f64 / sample.elapsed.as_secs_f64();
        eprintln!(
            "{label}: {rate:.1} logins/s ({} ok, {} connect, {} io, {} other, {:.1}s)",
            sample.ok, sample.connect, sample.io, sample.other, sample.elapsed.as_secs_f64()
        );
        rows.push((label, rate, sample));
    }

    println!();
    println!(
        "{:<16} {:>12} {:>10} {:>10} {:>10} {:>10}",
        "scenario", "logins/s", "ok", "connect", "io", "other"
    );
    for (label, rate, sample) in rows {
        println!(
            "{label:<16} {rate:>12.1} {:>10} {:>10} {:>10} {:>10}",
            sample.ok, sample.connect, sample.io, sample.other
        );
    }
    Ok(())
}

async fn seed(pool: &MySqlPool) -> anyhow::Result<()> {
    eprintln!("removing previous bench accounts");
    sqlx::query("DELETE FROM `accounts` WHERE `name` LIKE 'bench%'")
        .execute(pool)
        .await?;

    let hash = crypto::sha1_hex(PASSWORD.as_bytes());
    eprintln!("inserting {ACCOUNTS} accounts");
    let mut start = 0u32;
    while start < ACCOUNTS {
        let count = (ACCOUNTS - start).min(BATCH);
        insert_accounts(pool, start, count, &hash).await?;
        start += count;
        if start % 10_000 == 0 || start == ACCOUNTS {
            eprintln!("  accounts {start}/{ACCOUNTS}");
        }
    }

    let rows = sqlx::query("SELECT `id`, `name` FROM `accounts` WHERE `name` LIKE 'bench%'")
        .fetch_all(pool)
        .await?;
    if rows.len() != ACCOUNTS as usize {
        anyhow::bail!("expected {ACCOUNTS} bench accounts, found {}", rows.len());
    }
    let mut ids = vec![0u32; ACCOUNTS as usize];
    for row in rows {
        let id: i32 = row.try_get("id")?;
        let name: String = row.try_get("name")?;
        let index: u32 = name
            .strip_prefix("bench")
            .and_then(|rest| rest.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("unexpected account name {name}"))?;
        if index >= ACCOUNTS {
            anyhow::bail!("account index {index} is outside the bench range");
        }
        ids[index as usize] = u32::try_from(id)?;
    }

    eprintln!("inserting {} characters", ACCOUNTS * 3);
    let mut batch: Vec<(String, u32)> = Vec::with_capacity(BATCH as usize);
    let mut inserted = 0u32;
    for (index, account_id) in ids.iter().copied().enumerate() {
        if account_id == 0 {
            anyhow::bail!("missing id for {}", account_name(index as u32));
        }
        for slot in 0..3u8 {
            batch.push((player_name(index as u32, slot), account_id));
            if batch.len() == BATCH as usize {
                insert_players(pool, &batch).await?;
                inserted += BATCH;
                batch.clear();
                if inserted % 30_000 == 0 {
                    eprintln!("  characters {inserted}/{}", ACCOUNTS * 3);
                }
            }
        }
    }
    if !batch.is_empty() {
        inserted += batch.len() as u32;
        insert_players(pool, &batch).await?;
    }
    eprintln!("seeded {ACCOUNTS} accounts and {inserted} characters");
    Ok(())
}

async fn insert_accounts(pool: &MySqlPool, start: u32, count: u32, hash: &str) -> anyhow::Result<()> {
    let mut sql = String::from("INSERT INTO `accounts` (`name`, `password`) VALUES ");
    for i in 0..count {
        if i > 0 {
            sql.push(',');
        }
        sql.push_str("(?,?)");
    }
    let mut query = sqlx::query(&sql);
    for i in 0..count {
        query = query.bind(account_name(start + i)).bind(hash);
    }
    query.execute(pool).await?;
    Ok(())
}

async fn insert_players(pool: &MySqlPool, rows: &[(String, u32)]) -> anyhow::Result<()> {
    let mut sql = String::from("INSERT INTO `players` (`name`, `account_id`) VALUES ");
    for i in 0..rows.len() {
        if i > 0 {
            sql.push(',');
        }
        sql.push_str("(?,?)");
    }
    let mut query = sqlx::query(&sql);
    for (name, account_id) in rows {
        query = query.bind(name).bind(i32::try_from(*account_id)?);
    }
    query.execute(pool).await?;
    Ok(())
}

fn account_name(index: u32) -> String {
    format!("bench{index:06}")
}

fn player_name(index: u32, slot: u8) -> String {
    let suffix = (b'a' + slot) as char;
    format!("Bench{index:06}{suffix}")
}

fn precompute(rsa: &RsaKey, names: &[String]) -> Vec<Vec<u8>> {
    let mut indexed = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(THREADS);
        for thread in 0..THREADS {
            let names = names;
            handles.push(scope.spawn(move || {
                let mut local = Vec::new();
                let mut index = thread;
                while index < names.len() {
                    let frame = protocol::build_login_frame(rsa, names[index].as_bytes(), PASSWORD.as_bytes())
                        .expect("login packet");
                    local.push((index, frame));
                    index += THREADS;
                }
                local
            }));
        }
        for handle in handles {
            indexed.extend(handle.join().expect("precompute thread"));
        }
    });
    indexed.sort_by_key(|(index, _)| *index);
    indexed.into_iter().map(|(_, frame)| frame).collect()
}

fn wait_port(addr: &str) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if TcpStream::connect(addr).is_ok() {
            eprintln!("{addr} is accepting connections");
            return Ok(());
        }
        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for {addr}");
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn wait_until(addr: &str, packet: &[u8], expect: LoginOutcome, limit: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + limit;
    loop {
        if matches!(attempt(addr, packet), Ok(kind) if kind == expect) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            anyhow::bail!("{addr} did not return {expect:?} within {}s", limit.as_secs());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

struct Sample {
    ok: u64,
    connect: u64,
    io: u64,
    other: u64,
    elapsed: Duration,
}

enum AttemptError {
    Connect,
    Io,
}

fn warmup(addr: &str, packets: &[Vec<u8>]) -> Sample {
    drive(addr, packets, None, LoginOutcome::CharacterList)
}

fn measure(addr: &str, packets: &[Vec<u8>], expect: LoginOutcome) -> Sample {
    drive(addr, packets, Some(Duration::from_secs(MEASURE_SECS)), expect)
}

fn drive(addr: &str, packets: &[Vec<u8>], duration: Option<Duration>, expect: LoginOutcome) -> Sample {
    let ok = AtomicU64::new(0);
    let connect = AtomicU64::new(0);
    let io = AtomicU64::new(0);
    let other = AtomicU64::new(0);
    let started = Instant::now();
    std::thread::scope(|scope| {
        for thread in 0..THREADS {
            let ok = &ok;
            let connect = &connect;
            let io = &io;
            let other = &other;
            scope.spawn(move || {
                let mut index = thread;
                loop {
                    if duration.is_some_and(|limit| started.elapsed() >= limit) {
                        break;
                    }
                    if duration.is_none() && index >= packets.len() {
                        break;
                    }
                    match attempt(addr, &packets[index % packets.len()]) {
                        Ok(kind) if kind == expect => {
                            ok.fetch_add(1, Ordering::Relaxed);
                        }
                        Ok(_) => {
                            other.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(AttemptError::Connect) => {
                            connect.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(AttemptError::Io) => {
                            io.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    index += THREADS;
                }
            });
        }
    });
    Sample {
        ok: ok.load(Ordering::Relaxed),
        connect: connect.load(Ordering::Relaxed),
        io: io.load(Ordering::Relaxed),
        other: other.load(Ordering::Relaxed),
        elapsed: started.elapsed(),
    }
}

fn attempt(addr: &str, packet: &[u8]) -> Result<LoginOutcome, AttemptError> {
    let mut stream = TcpStream::connect(addr).map_err(|_| AttemptError::Connect)?;
    stream.set_nodelay(true).ok();
    // RST on close instead of TIME_WAIT. Each login is a new TCP connection, and
    // tens of thousands of those otherwise exhaust the ephemeral port range.
    {
        let sock = socket2::Socket::from(stream);
        sock.set_linger(Some(Duration::ZERO)).ok();
        stream = sock.into();
    }
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(2))).ok();
    stream.write_all(packet).map_err(|_| AttemptError::Io)?;
    let mut header = [0u8; 2];
    stream.read_exact(&mut header).map_err(|_| AttemptError::Io)?;
    let size = u16::from_le_bytes(header) as usize;
    if size == 0 || size > protocol::MAX_BODY {
        return Err(AttemptError::Io);
    }
    let mut body = vec![0u8; size];
    stream.read_exact(&mut body).map_err(|_| AttemptError::Io)?;
    let mut full = Vec::with_capacity(2 + size);
    full.extend_from_slice(&header);
    full.extend_from_slice(&body);
    protocol::classify_sealed(&full, &XTEA_KEY).ok_or(AttemptError::Io)
}

fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}
