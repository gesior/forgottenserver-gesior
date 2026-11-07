mod cam;
mod config;
mod crypto;
mod db;
mod protocol;

use cam::loader::CamLoader;
use cam::session::PlaybackSession;
use config::Config;
use crypto::rsa::RsaDecryptor;
use crypto::xtea;
use db::repository::Repository;
use protocol::framer::PacketFramer;
use protocol::handshake::ChallengePacket;
use protocol::login::LoginParser;
use protocol::messages::{MessageBuilder, MESSAGE_STATUS_CONSOLE_BLUE, MESSAGE_STATUS_SMALL};

use byteorder::{LittleEndian, ReadBytesExt};
use sqlx::mysql::MySqlPoolOptions;
use std::io::Cursor;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::sleep;

const GAME_PACKET_COALESCE_LIMIT: usize = 16_384;

const CAM_CONTROLS_HELP: &str = "Controls: Ctrl+Up/Down change speed, Ctrl+Left/Right skip 10s. Commands: /goto <sec>, /back <sec>, /forward <sec>, /speed <mult>.";

/// The first world packet (opcode 0x17) resets the client, including the console.
/// Notices sent in the same burst are wiped, so they wait until that packet has been applied.
const LOGIN_NOTICE_DELAY: Duration = Duration::from_secs(1);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("[CamPlayer] Starting Cam Player Server in Rust...");

    let config = Config::load_or_default("config.json");
    println!("[CamPlayer] Listening on:   {}", config.listen_address);
    println!("[CamPlayer] Cams dir:       {}", config.cams_dir);
    println!("[CamPlayer] Disk cache:     {} MB", config.max_disk_cache_mb);
    println!(
        "[CamPlayer] Playback limits: {} normal, {} with access code",
        config.max_concurrent_playbacks, config.max_concurrent_access_code_playbacks
    );
    println!("[CamPlayer] RSA key path:   {}", config.rsa_key_path);
    println!("[CamPlayer] DB connection:  {}:{} (db: {})",
        config.database.host, config.database.port, config.database.name);

    let rsa = Arc::new(RsaDecryptor::from_pem_file(&config.rsa_key_path)
        .map_err(|e| format!("Failed to load RSA private key from {}: {}", config.rsa_key_path, e))?);

    println!("[CamPlayer] Connecting to MySQL database...");
    let pool = MySqlPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url())
        .await
        .map_err(|e| format!("Failed to connect to MySQL: {}", e))?;

    let repository = Arc::new(Repository::new(pool));
    let loader = Arc::new(CamLoader::new(
        &config.cams_dir,
        &config.cache_dir,
        config.max_disk_cache_bytes(),
    ));
    let slots = Arc::new(PlaybackSlots::new(
        config.max_concurrent_playbacks as usize,
        config.max_concurrent_access_code_playbacks as usize,
    ));

    let listener = TcpListener::bind(&config.listen_address).await
        .map_err(|e| format!("Failed to bind TCP listener to {}: {}", config.listen_address, e))?;

    println!("[CamPlayer] Ready! Accepting Tibia client connections on {}", config.listen_address);

    let mut shutdown = ShutdownSignals::install()?;

    loop {
        tokio::select! {
            biased;
            result = shutdown.recv() => {
                result?;
                break;
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((socket, addr)) => {
                        println!("[CamPlayer] New incoming connection from {}", addr);
                        let rsa_clone = rsa.clone();
                        let repo_clone = repository.clone();
                        let loader_clone = loader.clone();
                        let slots_clone = slots.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_client(socket, rsa_clone, repo_clone, loader_clone, slots_clone).await {
                                eprintln!("[CamPlayer] Connection {} closed with error: {}", addr, e);
                            } else {
                                println!("[CamPlayer] Connection {} closed gracefully.", addr);
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("[CamPlayer] Accept error: {}", e);
                        sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }

    Ok(())
}

struct ShutdownSignals {
    #[cfg(unix)]
    sigint: tokio::signal::unix::Signal,
    #[cfg(unix)]
    sigterm: tokio::signal::unix::Signal,
}

impl ShutdownSignals {
    fn install() -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            Ok(Self {
                sigint: signal(SignalKind::interrupt())?,
                sigterm: signal(SignalKind::terminate())?,
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }

    async fn recv(&mut self) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.sigint.recv() => {
                    println!("[CamPlayer] SIGINT, shutting down.");
                }
                _ = self.sigterm.recv() => {
                    println!("[CamPlayer] SIGTERM, shutting down.");
                }
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await?;
            println!("[CamPlayer] Ctrl+C, shutting down.");
            Ok(())
        }
    }
}

enum ClientEvent {
    Packet(Vec<u8>),
    Disconnected,
}

struct PlaybackSlots {
    active: Mutex<usize>,
    max_normal: usize,
    max_access_code: usize,
}

struct SessionPermit {
    slots: Arc<PlaybackSlots>,
}

impl PlaybackSlots {
    fn new(max_normal: usize, max_access_code: usize) -> Self {
        Self {
            active: Mutex::new(0),
            max_normal,
            max_access_code,
        }
    }

    fn try_acquire(self: &Arc<Self>, by_access_code: bool) -> Option<SessionPermit> {
        let mut active = lock_mutex(&self.active);
        let limit = if by_access_code { self.max_access_code } else { self.max_normal };
        if *active >= limit {
            return None;
        }
        *active += 1;
        Some(SessionPermit {
            slots: Arc::clone(self),
        })
    }
}

impl Drop for SessionPermit {
    fn drop(&mut self) {
        let mut active = lock_mutex(&self.slots.active);
        *active = active.saturating_sub(1);
    }
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

async fn handle_client(
    socket: TcpStream,
    rsa: Arc<RsaDecryptor>,
    repo: Arc<Repository>,
    loader: Arc<CamLoader>,
    slots: Arc<PlaybackSlots>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut reader, mut writer) = socket.into_split();

    // 1. Send Handshake 0x1F Challenge packet
    let challenge = ChallengePacket::new();
    use tokio::io::AsyncWriteExt;
    writer.write_all(&challenge.raw_bytes).await?;
    writer.flush().await?;

    // 2. Read Login packet (0x0A with RSA block)
    let raw_login = PacketFramer::read_raw_packet(&mut reader).await?;
    let login_info = match LoginParser::parse(&raw_login, &rsa) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("[CamPlayer] Login parse error: {}", e);
            return Ok(());
        }
    };

    println!(
        "[CamPlayer] Login received: Account='{}', Character='{}' -> Cam ID #{}",
        login_info.account, login_info.character_name, login_info.cam_id
    );

    // 3. Lookup Cam in Database
    let cam = match repo.get_cam_by_id(login_info.cam_id).await? {
        Some(m) => m,
        None => {
            println!("[CamPlayer] Cam ID #{} not found in database", login_info.cam_id);
            let err_pkt = MessageBuilder::error_message("Cam not found.");
            let _ = PacketFramer::send_packet(&mut writer, &err_pkt, &login_info.round_keys).await;
            return Ok(());
        }
    };

    // 4. Access code works for every cam. A public cam also plays with no password.
    //    A private cam needs the access code or the owning account's password.
    let by_access_code = !cam.access_code.is_empty() && cam.access_code == login_info.password;
    if cam.public == 0 && !by_access_code {
        let by_account = repo
            .verify_account_access(&login_info.account, &login_info.password, cam.account_id)
            .await?;
        if !by_account {
            println!("[CamPlayer] Unauthorized access attempt to private cam #{} by account '{}'", cam.id, login_info.account);
            let err_pkt = MessageBuilder::error_message("This recording is private or login credentials are invalid.");
            let _ = PacketFramer::send_packet(&mut writer, &err_pkt, &login_info.round_keys).await;
            return Ok(());
        }
    }

    let _permit = match slots.try_acquire(by_access_code) {
        Some(permit) => permit,
        None => {
            println!(
                "[CamPlayer] Playback limit reached for cam #{} (access code login: {})",
                cam.id, by_access_code
            );
            let err_pkt = MessageBuilder::error_message("Too many viewers are watching recordings. Try again later.");
            let _ = PacketFramer::send_packet(&mut writer, &err_pkt, &login_info.round_keys).await;
            return Ok(());
        }
    };

    // 5. Load recording archive
    let parsed_cam = match loader.load_cam(&cam.path).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[CamPlayer] Failed to load cam file '{}': {}", cam.path, e);
            let err_pkt = MessageBuilder::error_message("Failed to load cam recording file.");
            let _ = PacketFramer::send_packet(&mut writer, &err_pkt, &login_info.round_keys).await;
            return Ok(());
        }
    };

    println!(
        "[CamPlayer] Cam loaded: duration={}s, total packets={}, sync snapshots={}",
        parsed_cam.duration_ms / 1000,
        parsed_cam.packets.len(),
        parsed_cam.sync_snapshots.len()
    );

    // 6. Initialize Playback Session
    let mut session = PlaybackSession::new(parsed_cam, cam.offset_start, cam.offset_end);

    // Fast-seek to initial offset_start
    let init_packets = session.seek_to(session.offset_start_ms);
    send_game_packets(&mut writer, &init_packets, &login_info.round_keys).await?;

    let character_name = session.cam.character_name.trim();
    let character_name = if character_name.is_empty() {
        "Unknown"
    } else {
        character_name
    };
    let welcome = format!(
        "Playing: {} (Level {}). Duration: {}.",
        character_name,
        cam.player_level,
        session.visible_duration_label()
    );
    // Start the wait only after the initial world burst is on the wire.
    let mut login_notice_at = Instant::now() + LOGIN_NOTICE_DELAY;
    let mut login_notice_pending = true;

    // 7. Spawn background task to read client incoming commands
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<ClientEvent>(32);
    let round_keys_read = login_info.round_keys;

    tokio::spawn(async move {
        loop {
            match PacketFramer::read_decrypted_packet(&mut reader, &round_keys_read).await {
                Ok(data) => {
                    if cmd_tx.send(ClientEvent::Packet(data)).await.is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = cmd_tx.send(ClientEvent::Disconnected).await;
                    break;
                }
            }
        }
    });

    // 8. Main Streaming & Event Loop
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Real time, not cam time. The first tick fires immediately.
    let mut status_ticker = tokio::time::interval(Duration::from_secs(1));
    status_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let (packets, finished) = session.step();

                if !packets.is_empty() {
                    send_game_packets(&mut writer, &packets, &login_info.round_keys).await?;
                }

                if login_notice_pending && Instant::now() >= login_notice_at {
                    send_login_notices(&mut writer, &login_info.round_keys, &welcome).await?;
                    login_notice_pending = false;
                }

                if finished {
                    if login_notice_pending {
                        let remaining = login_notice_at.saturating_duration_since(Instant::now());
                        if !remaining.is_zero() {
                            sleep(remaining).await;
                        }
                        send_login_notices(&mut writer, &login_info.round_keys, &welcome).await?;
                    }
                    println!("[CamPlayer] Cam #{} playback reached end or offset_end limit.", cam.id);
                    let finished_msg = MessageBuilder::text_message("------- CAM FINISHED -------", 19);
                    let _ = PacketFramer::send_packet(&mut writer, &finished_msg, &login_info.round_keys).await;
                    sleep(Duration::from_millis(1500)).await;
                    let end_msg = MessageBuilder::error_message("Recording finished.");
                    let _ = PacketFramer::send_packet(&mut writer, &end_msg, &login_info.round_keys).await;
                    break;
                }
            }

            _ = status_ticker.tick() => {
                send_playback_status(&session, &mut writer, &login_info.round_keys).await?;
            }

            Some(event) = cmd_rx.recv() => {
                match event {
                    ClientEvent::Disconnected => {
                        println!("[CamPlayer] Client disconnected.");
                        break;
                    }
                    ClientEvent::Packet(data) => {
                        if data.is_empty() {
                            continue;
                        }
                        let opcode = data[0];
                        match opcode {
                            0x14 => {
                                // Client requested logout
                                println!("[CamPlayer] Client requested logout (0x14)");
                                break;
                            }
                            0x1D => {
                                // Ping -> send pong
                                let pong = MessageBuilder::pong();
                                PacketFramer::send_packet(&mut writer, &pong, &login_info.round_keys).await?;
                            }
                            0x96 => {
                                // Say packet: [0x96] [type: u8] [len: u16] [text]
                                if data.len() >= 4 {
                                    let mut cur = Cursor::new(&data[2..4]);
                                    if let Ok(text_len) = cur.read_u16::<LittleEndian>() {
                                        let text_len = text_len as usize;
                                        if 4 + text_len <= data.len() {
                                            let text = String::from_utf8_lossy(&data[4..4 + text_len]).to_lowercase();
                                            let reloads_world = command_reloads_world(&text);
                                            handle_chat_command(&text, &mut session, &mut writer, &login_info.round_keys).await?;
                                            if login_notice_pending && reloads_world {
                                                login_notice_at = Instant::now() + LOGIN_NOTICE_DELAY;
                                            }
                                        }
                                    }
                                }
                            }
                            // Hotkeys / turn commands
                            0x6F => {
                                // Turn North -> Speed Up (2x)
                                session.set_speed(session.speed * 2.0);
                                let cancel = MessageBuilder::cancel_walk();
                                PacketFramer::send_packet(&mut writer, &cancel, &login_info.round_keys).await?;
                                let msg = MessageBuilder::text_message(&format!("Speed: {:.2}x", session.speed), 19);
                                PacketFramer::send_packet(&mut writer, &msg, &login_info.round_keys).await?;
                                send_playback_status(&session, &mut writer, &login_info.round_keys).await?;
                            }
                            0x71 => {
                                // Turn South -> Slow Down (0.5x)
                                session.set_speed(session.speed / 2.0);
                                let cancel = MessageBuilder::cancel_walk();
                                PacketFramer::send_packet(&mut writer, &cancel, &login_info.round_keys).await?;
                                let msg = MessageBuilder::text_message(&format!("Speed: {:.2}x", session.speed), 19);
                                PacketFramer::send_packet(&mut writer, &msg, &login_info.round_keys).await?;
                                send_playback_status(&session, &mut writer, &login_info.round_keys).await?;
                            }
                            0x70 => {
                                // Turn East -> Skip Forward 10s
                                let target = session.virtual_time_ms + 10_000;
                                let burst = session.seek_to(target);
                                let cancel = MessageBuilder::cancel_walk();
                                PacketFramer::send_packet(&mut writer, &cancel, &login_info.round_keys).await?;
                                send_game_packets(&mut writer, &burst, &login_info.round_keys).await?;
                                if login_notice_pending {
                                    login_notice_at = Instant::now() + LOGIN_NOTICE_DELAY;
                                }
                                let msg = MessageBuilder::text_message(&format!("Skipped forward to {}s", session.virtual_time_ms / 1000), 19);
                                PacketFramer::send_packet(&mut writer, &msg, &login_info.round_keys).await?;
                                send_playback_status(&session, &mut writer, &login_info.round_keys).await?;
                            }
                            0x72 => {
                                // Turn West -> Skip Backward 10s
                                let target = session.virtual_time_ms.saturating_sub(10_000);
                                let burst = session.seek_to(target);
                                let cancel = MessageBuilder::cancel_walk();
                                PacketFramer::send_packet(&mut writer, &cancel, &login_info.round_keys).await?;
                                send_game_packets(&mut writer, &burst, &login_info.round_keys).await?;
                                if login_notice_pending {
                                    login_notice_at = Instant::now() + LOGIN_NOTICE_DELAY;
                                }
                                let msg = MessageBuilder::text_message(&format!("Skipped backward to {}s", session.virtual_time_ms / 1000), 19);
                                PacketFramer::send_packet(&mut writer, &msg, &login_info.round_keys).await?;
                                send_playback_status(&session, &mut writer, &login_info.round_keys).await?;
                            }
                            0x65 | 0x66 | 0x67 | 0x68 => {
                                // Arrow walking -> reply cancel walk
                                let cancel = MessageBuilder::cancel_walk();
                                PacketFramer::send_packet(&mut writer, &cancel, &login_info.round_keys).await?;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_chat_command(
    text: &str,
    session: &mut PlaybackSession,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    round_keys: &xtea::RoundKeys,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }

    let cmd = parts[0];
    match cmd {
        "/goto" | "/time" => {
            if let Some(sec_str) = parts.get(1) {
                if let Ok(sec) = sec_str.parse::<u64>() {
                    let burst = session.seek_to(sec * 1000);
                    send_game_packets(writer, &burst, round_keys).await?;
                    let resp = MessageBuilder::text_message(&format!("Jumped to {}s", sec), 19);
                    PacketFramer::send_packet(writer, &resp, round_keys).await?;
                    send_playback_status(session, writer, round_keys).await?;
                }
            }
        }
        "/forward" | "/next" => {
            if let Some(sec_str) = parts.get(1) {
                if let Ok(sec) = sec_str.parse::<u64>() {
                    let target = session.virtual_time_ms + (sec * 1000);
                    let burst = session.seek_to(target);
                    send_game_packets(writer, &burst, round_keys).await?;
                    let resp = MessageBuilder::text_message(&format!("Jumped forward to {}s", session.virtual_time_ms / 1000), 19);
                    PacketFramer::send_packet(writer, &resp, round_keys).await?;
                    send_playback_status(session, writer, round_keys).await?;
                }
            }
        }
        "/back" | "/backward" => {
            if let Some(sec_str) = parts.get(1) {
                if let Ok(sec) = sec_str.parse::<u64>() {
                    let target = session.virtual_time_ms.saturating_sub(sec * 1000);
                    let burst = session.seek_to(target);
                    send_game_packets(writer, &burst, round_keys).await?;
                    let resp = MessageBuilder::text_message(&format!("Jumped back to {}s", session.virtual_time_ms / 1000), 19);
                    PacketFramer::send_packet(writer, &resp, round_keys).await?;
                    send_playback_status(session, writer, round_keys).await?;
                }
            }
        }
        "/speed" => {
            if let Some(speed_str) = parts.get(1) {
                if let Ok(speed) = speed_str.parse::<f64>() {
                    session.set_speed(speed);
                    let resp = MessageBuilder::text_message(&format!("Speed set to {:.2}x", session.speed), 19);
                    PacketFramer::send_packet(writer, &resp, round_keys).await?;
                    send_playback_status(session, writer, round_keys).await?;
                }
            }
        }
        "/help" => {
            let resp = MessageBuilder::text_message(CAM_CONTROLS_HELP, MESSAGE_STATUS_CONSOLE_BLUE);
            PacketFramer::send_packet(writer, &resp, round_keys).await?;
        }
        _ => {}
    }

    Ok(())
}

/// Sends game messages in XTEA frames of at most 16 KB.
/// A single record larger than that limit (a full map description) goes out as its own frame.
async fn send_game_packets(
    writer: &mut OwnedWriteHalf,
    packets: &[Vec<u8>],
    round_keys: &xtea::RoundKeys,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut coalesce_buffer = Vec::new();
    for payload in packets {
        if payload.is_empty() {
            continue;
        }
        if !coalesce_buffer.is_empty()
            && coalesce_buffer.len() + payload.len() > GAME_PACKET_COALESCE_LIMIT
        {
            PacketFramer::send_packet(writer, &coalesce_buffer, round_keys).await?;
            coalesce_buffer.clear();
        }
        coalesce_buffer.extend_from_slice(payload);
    }
    if !coalesce_buffer.is_empty() {
        PacketFramer::send_packet(writer, &coalesce_buffer, round_keys).await?;
    }
    Ok(())
}

fn command_reloads_world(text: &str) -> bool {
    matches!(
        text.split_whitespace().next(),
        Some("/goto" | "/time" | "/forward" | "/next" | "/back" | "/backward")
    )
}

async fn send_login_notices(
    writer: &mut OwnedWriteHalf,
    round_keys: &xtea::RoundKeys,
    welcome: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let welcome_msg = MessageBuilder::text_message(welcome, MESSAGE_STATUS_CONSOLE_BLUE);
    PacketFramer::send_packet(writer, &welcome_msg, round_keys).await?;
    let help_msg = MessageBuilder::text_message(CAM_CONTROLS_HELP, MESSAGE_STATUS_CONSOLE_BLUE);
    PacketFramer::send_packet(writer, &help_msg, round_keys).await?;
    Ok(())
}

async fn send_playback_status(
    session: &PlaybackSession,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    round_keys: &xtea::RoundKeys,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if session.is_finished {
        return Ok(());
    }

    let status_msg = MessageBuilder::text_message(&session.status_line(), MESSAGE_STATUS_SMALL);
    PacketFramer::send_packet(writer, &status_msg, round_keys).await?;
    Ok(())
}
