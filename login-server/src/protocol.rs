// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

use crate::cache::{name_key, unix_now, Snapshot};
use crate::config::{Config, ServerStatus};
use crate::crypto::{self, RoundKeys, RSA_BLOCK};
use chrono::Datelike;

pub const MAX_BODY: usize = 24590 - 16;

#[derive(Debug, Clone, Copy)]
pub struct ConnMeta {
    pub ip: u32,
    pub otc_proxy: bool,
    pub ha_proxy: bool,
}

pub struct ResponseParts {
    pub round_keys: RoundKeys,
    pub account: Vec<u8>,
    pub password: Vec<u8>,
    pub token: Vec<u8>,
    pub ticks: u32,
    pub premium_ends_at: u32,
    pub authenticated_2fa: bool,
    pub otc_proxy: bool,
    pub ha_proxy: bool,
    pub version: u16,
}

pub enum Phase1 {
    Silence,
    Reject(Vec<u8>),
    Accept(Vec<u8>),
    NeedList {
        account_id: u32,
        parts: ResponseParts,
    },
}

pub fn authenticate(
    body: &[u8],
    meta: ConnMeta,
    cfg: &Config,
    rsa: &crypto::RsaKey,
    accounts: &Snapshot,
) -> Phase1 {
    if body.len() < 5 {
        return Phase1::Silence;
    }
    let recv_sum = u32::from_le_bytes(body[0..4].try_into().unwrap());
    if recv_sum != crypto::adler32(&body[4..]) {
        return Phase1::Silence;
    }
    let payload = &body[4..];
    if payload[0] != 0x01 || payload.len() < 5 {
        return Phase1::Silence;
    }
    let version = u16::from_le_bytes(payload[3..5].try_into().unwrap());
    if version <= 760 {
        let message = format!(
            "Only clients with protocol {} allowed!",
            cfg.server.client_version_string
        );
        return Phase1::Reject(seal_plain(&error_body(version, &message)));
    }

    let skip = if version >= 971 { 17 } else { 12 };
    let rsa_at = 1 + 2 + 2 + skip;
    if payload.len() < rsa_at + RSA_BLOCK {
        return Phase1::Silence;
    }
    let mut plain_payload = payload.to_vec();
    let Some(block) = rsa.decrypt_block(&plain_payload[rsa_at..rsa_at + RSA_BLOCK]) else {
        return Phase1::Silence;
    };
    plain_payload[rsa_at..rsa_at + RSA_BLOCK].copy_from_slice(&block);

    let mut pos = rsa_at + 1;
    let Some(k0) = read_u32(&plain_payload, &mut pos) else {
        return Phase1::Silence;
    };
    let Some(k1) = read_u32(&plain_payload, &mut pos) else {
        return Phase1::Silence;
    };
    let Some(k2) = read_u32(&plain_payload, &mut pos) else {
        return Phase1::Silence;
    };
    let Some(k3) = read_u32(&plain_payload, &mut pos) else {
        return Phase1::Silence;
    };
    let round_keys = crypto::expand_key([k0, k1, k2, k3]);
    let account = read_string(&plain_payload, &mut pos).unwrap_or_default();
    let password = read_string(&plain_payload, &mut pos).unwrap_or_default();

    let reject = |message: &str| Phase1::Reject(seal_xtea(&error_body(version, message), &round_keys));

    if version < cfg.server.client_version_min || version > cfg.server.client_version_max {
        let message = format!(
            "Only clients with protocol {} allowed!",
            cfg.server.client_version_string
        );
        return reject(&message);
    }
    match cfg.server.status {
        ServerStatus::Starting => {
            return reject("Gameworld is starting up. Please wait.");
        }
        ServerStatus::Maintenance => {
            return reject("Gameworld is under maintenance.\nPlease re-connect in a while.");
        }
        ServerStatus::Online => {}
    }
    if meta.ip != 0 {
        if let Some(ban) = accounts.ip_bans.get(&meta.ip) {
            let now = unix_now() as i64;
            if ban.expires_at == 0 || now <= ban.expires_at {
                let reason = if ban.reason.is_empty() {
                    "(none)"
                } else {
                    ban.reason.as_str()
                };
                let message = format!(
                    "Your IP has been banned until {} by {}.\n\nReason specified:\n{}",
                    format_date_short(ban.expires_at),
                    ban.banned_by,
                    reason
                );
                return reject(&message);
            }
        }
    }
    if account.is_empty() {
        return reject("Invalid account name.");
    }
    if password.is_empty() {
        return reject("Invalid password.");
    }
    if body.len() < RSA_BLOCK {
        return Phase1::Silence;
    }
    let rsa2_at = body.len() - RSA_BLOCK;
    let Some(token_block) = rsa.decrypt_block(&body[rsa2_at..rsa2_at + RSA_BLOCK]) else {
        return reject("Invalid authentication token.");
    };
    let mut token_pos = 1;
    let token = read_string(&token_block, &mut token_pos).unwrap_or_default();

    let Some(&account_id) = accounts.by_name.get(&name_key(&account)) else {
        return reject("Account name or password is not correct.");
    };
    let Some(record) = accounts.by_id.get(&account_id) else {
        return reject("Account name or password is not correct.");
    };
    if !crypto::sha1_hex(&password).eq_ignore_ascii_case(&record.password_sha1) {
        return reject("Account name or password is not correct.");
    }

    let ticks = (unix_now() / 30) as u32;
    let mut authenticated_2fa = false;
    if !record.secret.is_empty() {
        if !crypto::token_matches(&record.secret, &token, ticks) {
            return Phase1::Reject(seal_xtea(&[0x0D, 0x00], &round_keys));
        }
        authenticated_2fa = true;
    }

    Phase1::NeedList {
        account_id,
        parts: ResponseParts {
            round_keys,
            account,
            password,
            token,
            ticks,
            premium_ends_at: record.premium_ends_at,
            authenticated_2fa,
            otc_proxy: meta.otc_proxy,
            ha_proxy: meta.ha_proxy,
            version,
        },
    }
}

pub fn seal_character_list(
    cfg: &Config,
    snap: &Snapshot,
    parts: &ResponseParts,
    names: &[String],
) -> Vec<u8> {
    let mut body = Vec::with_capacity(128 + names.len() * 16);
    if parts.authenticated_2fa {
        body.extend_from_slice(&[0x0C, 0x00]);
    }
    if !cfg.server.motd.is_empty() {
        body.push(0x14);
        let text = format!("{}\n{}", snap.motd_num, cfg.server.motd);
        write_string(&mut body, text.as_bytes());
    }
    body.push(0x28);
    let mut session = Vec::with_capacity(parts.account.len() + parts.password.len() + parts.token.len() + 16);
    session.extend_from_slice(&parts.account);
    session.push(b'\n');
    session.extend_from_slice(&parts.password);
    session.push(b'\n');
    session.extend_from_slice(&parts.token);
    session.push(b'\n');
    session.extend_from_slice(parts.ticks.to_string().as_bytes());
    write_string(&mut body, &session);

    body.push(0x64);
    let ip = world_ip(cfg, parts);
    if cfg.server.show_online_status {
        body.push(2);
        for world_id in 0..2u8 {
            body.push(world_id);
            write_string(&mut body, if world_id == 0 { b"Offline" } else { b"Online" });
            write_string(&mut body, ip.as_bytes());
            body.extend_from_slice(&cfg.server.game_port.to_le_bytes());
            body.push(0);
        }
    } else {
        body.push(1);
        body.push(0);
        write_string(&mut body, cfg.server.name.as_bytes());
        write_string(&mut body, ip.as_bytes());
        body.extend_from_slice(&cfg.server.game_port.to_le_bytes());
        body.push(0);
    }

    let count = names.len().min(u8::MAX as usize);
    body.push(count as u8);
    for name in names.iter().take(count) {
        let world_id = if cfg.server.show_online_status && snap.online_names.contains(name) {
            1
        } else {
            0
        };
        body.push(world_id);
        write_string(&mut body, name.as_bytes());
    }

    body.push(0);
    if cfg.server.free_premium {
        body.push(1);
        body.extend_from_slice(&0u32.to_le_bytes());
    } else {
        let premium = u64::from(parts.premium_ends_at) > unix_now();
        body.push(u8::from(premium));
        body.extend_from_slice(&parts.premium_ends_at.to_le_bytes());
    }
    seal_xtea(&body, &parts.round_keys)
}

pub fn error_body(version: u16, message: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(3 + message.len());
    body.push(if version >= 1076 { 0x0B } else { 0x0A });
    write_string(&mut body, message.as_bytes());
    body
}

pub fn seal_xtea(body: &[u8], key: &RoundKeys) -> Vec<u8> {
    let mut inner = Vec::with_capacity(body.len() + 10);
    inner.extend_from_slice(&(body.len() as u16).to_le_bytes());
    inner.extend_from_slice(body);
    let pad = (8 - (inner.len() % 8)) % 8;
    inner.extend(std::iter::repeat(0x33).take(pad));
    crypto::xtea_encrypt(&mut inner, key);
    let sum = crypto::adler32(&inner);
    let mut out = Vec::with_capacity(6 + inner.len());
    out.extend_from_slice(&((4 + inner.len()) as u16).to_le_bytes());
    out.extend_from_slice(&sum.to_le_bytes());
    out.extend_from_slice(&inner);
    out
}

pub fn seal_plain(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + body.len());
    out.extend_from_slice(&(body.len() as u16).to_le_bytes());
    out.extend_from_slice(body);
    out
}

pub fn write_string(out: &mut Vec<u8>, value: &[u8]) {
    let len = value.len().min(u16::MAX as usize);
    out.extend_from_slice(&(len as u16).to_le_bytes());
    out.extend_from_slice(&value[..len]);
}

fn world_ip<'a>(cfg: &'a Config, parts: &ResponseParts) -> &'a str {
    if parts.otc_proxy {
        "127.0.0.1"
    } else if parts.ha_proxy {
        cfg.server.status_ip.as_str()
    } else {
        cfg.server.ip.as_str()
    }
}

fn read_u32(buf: &[u8], pos: &mut usize) -> Option<u32> {
    if *pos + 4 > buf.len() {
        return None;
    }
    let value = u32::from_le_bytes(buf[*pos..*pos + 4].try_into().ok()?);
    *pos += 4;
    Some(value)
}

fn read_string(buf: &[u8], pos: &mut usize) -> Option<Vec<u8>> {
    if *pos + 2 > buf.len() {
        return None;
    }
    let len = u16::from_le_bytes(buf[*pos..*pos + 2].try_into().ok()?) as usize;
    *pos += 2;
    if *pos + len > buf.len() {
        return None;
    }
    let value = buf[*pos..*pos + len].to_vec();
    *pos += len;
    Some(value)
}

fn format_date_short(timestamp: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or(chrono::DateTime::UNIX_EPOCH);
    let local = utc.with_timezone(&chrono::Local);
    let month = MONTHS[local.month0() as usize];
    format!("{:02} {} {}", local.day(), month, local.year())
}

/// TFS on little-endian stores `htonl(ipv4)` in `ip_bans.ip`.
pub fn tfs_ipv4(octets: [u8; 4]) -> u32 {
    u32::from_le_bytes(octets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{AccountUpdate, IpBan};
    use crate::crypto::RsaKey;
    use std::collections::HashSet;
    use std::net::Ipv4Addr;
    use std::sync::OnceLock;

    fn key() -> &'static RsaKey {
        static KEY: OnceLock<RsaKey> = OnceLock::new();
        KEY.get_or_init(|| RsaKey::from_pem(include_bytes!("../tests/fixtures/key.pem")).unwrap())
    }

    fn cfg(status: &str) -> Config {
        Config::from_toml(&format!(
            r#"
bind = "0.0.0.0:7173"
rsa_key_file = "key.pem"
[server]
name = "Forgotten"
ip = "10.0.0.5"
status_ip = "10.0.0.9"
game_port = 7172
motd = "Hello"
status = "{status}"
[mysql]
host = "127.0.0.1"
user = "forgottenserver"
database = "forgottenserver"
[cache]
"#
        ))
        .unwrap()
    }

    fn rsa_plain(content: &[u8]) -> [u8; RSA_BLOCK] {
        let mut plain = [0u8; RSA_BLOCK];
        plain[..content.len()].copy_from_slice(content);
        plain
    }

    fn login_body(version: u16, first: &[u8], second: &[u8]) -> Vec<u8> {
        let enc1 = key().encrypt_block(&rsa_plain(first)).unwrap();
        let enc2 = key().encrypt_block(&rsa_plain(second)).unwrap();
        let mut payload = Vec::new();
        payload.push(0x01);
        payload.extend_from_slice(&2u16.to_le_bytes());
        payload.extend_from_slice(&version.to_le_bytes());
        payload.extend(std::iter::repeat(0).take(17));
        payload.extend_from_slice(&enc1);
        payload.extend_from_slice(&enc2);
        let mut body = Vec::new();
        body.extend_from_slice(&crypto::adler32(&payload).to_le_bytes());
        body.extend_from_slice(&payload);
        body
    }

    fn credentials(account: &[u8], password: &[u8]) -> Vec<u8> {
        let mut block = vec![0u8];
        block.extend_from_slice(&1u32.to_le_bytes());
        block.extend_from_slice(&2u32.to_le_bytes());
        block.extend_from_slice(&3u32.to_le_bytes());
        block.extend_from_slice(&4u32.to_le_bytes());
        write_string(&mut block, account);
        write_string(&mut block, password);
        block
    }

    fn token_block(token: &[u8]) -> Vec<u8> {
        let mut block = vec![0u8];
        write_string(&mut block, token);
        block
    }

    fn account_snapshot(password: &str) -> Snapshot {
        let mut snap = Snapshot::default();
        snap.motd_num = 7;
        snap.upsert(AccountUpdate {
            id: 4,
            name: "Admin".to_owned(),
            password_sha1: crypto::sha1_hex(password.as_bytes()),
            secret: Vec::new(),
            premium_ends_at: 0,
        });
        snap
    }

    fn open_sealed(packet: &[u8]) -> Vec<u8> {
        assert!(packet.len() >= 6);
        let declared = u16::from_le_bytes(packet[0..2].try_into().unwrap()) as usize;
        assert_eq!(declared, packet.len() - 2);
        let encrypted = &packet[6..];
        assert_eq!(
            crypto::adler32(encrypted),
            u32::from_le_bytes(packet[2..6].try_into().unwrap())
        );
        assert_eq!(encrypted.len() % 8, 0);
        let key = crypto::expand_key([1, 2, 3, 4]);
        let mut data = encrypted.to_vec();
        crypto::xtea_decrypt(&mut data, &key);
        let inner_len = u16::from_le_bytes(data[0..2].try_into().unwrap()) as usize;
        assert!(2 + inner_len <= data.len());
        for byte in &data[2 + inner_len..] {
            assert_eq!(*byte, 0x33);
        }
        data[2..2 + inner_len].to_vec()
    }

    #[test]
    fn tfs_ip_matches_little_endian_htonl() {
        assert_eq!(tfs_ipv4(Ipv4Addr::new(127, 0, 0, 1).octets()), 0x0100_007f);
    }

    #[test]
    fn bad_checksum_is_silent() {
        let cfg = cfg("online");
        let snap = Snapshot::default();
        let phase = authenticate(b"\x01\x00\x00\x00\x01", ConnMeta { ip: 0, otc_proxy: false, ha_proxy: false }, &cfg, key(), &snap);
        assert!(matches!(phase, Phase1::Silence));
    }

    #[test]
    fn old_client_gets_plaintext_error() {
        let cfg = cfg("online");
        let mut payload = vec![0x01, 0, 0];
        payload.extend_from_slice(&760u16.to_le_bytes());
        let mut body = Vec::new();
        body.extend_from_slice(&crypto::adler32(&payload).to_le_bytes());
        body.extend_from_slice(&payload);
        let phase = authenticate(&body, ConnMeta { ip: 0, otc_proxy: false, ha_proxy: false }, &cfg, key(), &Snapshot::default());
        let Phase1::Reject(packet) = phase else {
            panic!("expected plaintext reject");
        };
        assert_eq!(packet[2], 0x0A);
        let text = &packet[5..];
        assert!(std::str::from_utf8(text).unwrap().contains("10.98"));
    }

    #[test]
    fn valid_login_lists_characters_without_touching_plaintext_password_in_db_name() {
        let cfg = cfg("online");
        let snap = account_snapshot("secret");
        let body = login_body(1098, &credentials(b"admin", b"secret"), &token_block(b""));
        let phase = authenticate(
            &body,
            ConnMeta { ip: 0, otc_proxy: false, ha_proxy: false },
            &cfg,
            key(),
            &snap,
        );
        let Phase1::NeedList { account_id, parts } = phase else {
            panic!("expected character list");
        };
        assert_eq!(account_id, 4);
        assert!(!parts.authenticated_2fa);
        let names = ["Beta".to_owned(), "Zeta".to_owned()];
        let packet = seal_character_list(&cfg, &snap, &parts, &names);
        let opened = open_sealed(&packet);
        assert_eq!(opened[0], 0x14);
        assert!(opened.windows(7).any(|window| window == b"7\nHello"));
        assert!(opened.windows(b"admin\nsecret\n".len()).any(|window| window == b"admin\nsecret\n"));
        assert!(opened.contains(&0x64));
        assert!(opened.windows(4).any(|window| window == b"Beta"));
        assert!(opened.windows(4).any(|window| window == b"Zeta"));
        assert_eq!(&opened[opened.len() - 6..], &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn wrong_password_is_xtea_error() {
        let cfg = cfg("online");
        let snap = account_snapshot("secret");
        let body = login_body(1098, &credentials(b"admin", b"nope"), &token_block(b""));
        let phase = authenticate(
            &body,
            ConnMeta { ip: 0, otc_proxy: false, ha_proxy: false },
            &cfg,
            key(),
            &snap,
        );
        let Phase1::Reject(packet) = phase else {
            panic!("expected reject");
        };
        let opened = open_sealed(&packet);
        assert_eq!(opened[0], 0x0B);
        let text = std::str::from_utf8(&opened[3..]).unwrap();
        assert_eq!(text, "Account name or password is not correct.");
    }

    #[test]
    fn maintenance_is_encrypted() {
        let cfg = cfg("maintenance");
        let body = login_body(1098, &credentials(b"admin", b"secret"), &token_block(b""));
        let phase = authenticate(
            &body,
            ConnMeta { ip: 0, otc_proxy: false, ha_proxy: false },
            &cfg,
            key(),
            &Snapshot::default(),
        );
        let Phase1::Reject(packet) = phase else {
            panic!("expected reject");
        };
        let opened = open_sealed(&packet);
        let text = std::str::from_utf8(&opened[3..]).unwrap();
        assert!(text.contains("maintenance"));
    }

    #[test]
    fn ip_ban_uses_tfs_integer() {
        let cfg = cfg("online");
        let mut snap = Snapshot::default();
        snap.ip_bans.insert(
            tfs_ipv4([127, 0, 0, 1]),
            IpBan {
                expires_at: 0,
                reason: String::new(),
                banned_by: "God".to_owned(),
            },
        );
        let body = login_body(1098, &credentials(b"admin", b"secret"), &token_block(b""));
        let phase = authenticate(
            &body,
            ConnMeta {
                ip: tfs_ipv4([127, 0, 0, 1]),
                otc_proxy: false,
                ha_proxy: false,
            },
            &cfg,
            key(),
            &snap,
        );
        let Phase1::Reject(packet) = phase else {
            panic!("expected ban");
        };
        let opened = open_sealed(&packet);
        let text = std::str::from_utf8(&opened[3..]).unwrap();
        assert!(text.contains("Your IP has been banned until "));
        assert!(text.contains("by God"));
        assert!(text.contains("(none)"));
    }

    #[test]
    fn bad_authenticator_is_0x0d_and_good_one_prefixes_0x0c() {
        let cfg = cfg("online");
        let secret = b"12345678901234567890";
        let ticks = (unix_now() / 30) as u32;
        let good = crypto::hotp(secret, u64::from(ticks)).unwrap();
        let mut snap = account_snapshot("secret");
        snap.upsert(AccountUpdate {
            id: 4,
            name: "Admin".to_owned(),
            password_sha1: crypto::sha1_hex(b"secret"),
            secret: secret.to_vec(),
            premium_ends_at: 0,
        });
        let valid: Vec<String> = [ticks, ticks.wrapping_sub(1), ticks.wrapping_add(1)]
            .into_iter()
            .map(|counter| crypto::hotp(secret, u64::from(counter)).unwrap())
            .collect();
        let bad_token = ["000000", "111111", "999999"]
            .into_iter()
            .find(|candidate| !valid.iter().any(|code| code == *candidate))
            .unwrap();
        let bad = login_body(
            1098,
            &credentials(b"admin", b"secret"),
            &token_block(bad_token.as_bytes()),
        );
        let phase = authenticate(
            &bad,
            ConnMeta { ip: 0, otc_proxy: false, ha_proxy: false },
            &cfg,
            key(),
            &snap,
        );
        let Phase1::Reject(packet) = phase else {
            panic!("expected token reject");
        };
        assert_eq!(open_sealed(&packet), vec![0x0D, 0x00]);

        let good_body = login_body(1098, &credentials(b"admin", b"secret"), &token_block(good.as_bytes()));
        let phase = authenticate(
            &good_body,
            ConnMeta { ip: 1, otc_proxy: true, ha_proxy: false },
            &cfg,
            key(),
            &snap,
        );
        let Phase1::NeedList { parts, .. } = phase else {
            panic!("expected list");
        };
        assert!(parts.authenticated_2fa);
        let mut online = HashSet::new();
        online.insert("Beta".to_owned());
        let mut snap = snap;
        snap.online_names = online;
        let mut cfg = cfg;
        cfg.server.show_online_status = true;
        let packet = seal_character_list(&cfg, &snap, &parts, &["Beta".to_owned()]);
        let opened = open_sealed(&packet);
        assert_eq!(&opened[..2], &[0x0C, 0x00]);
        assert!(opened.windows(7).any(|window| window == b"Offline"));
        assert!(opened.windows(9).any(|window| window == b"127.0.0.1"));
    }
}
