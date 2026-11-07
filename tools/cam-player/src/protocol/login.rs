use crate::crypto::adler32::adler32;
use crate::crypto::rsa::RsaDecryptor;
use crate::crypto::xtea::{self, Key, RoundKeys};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

#[derive(Debug, Clone)]
pub struct LoginInfo {
    #[allow(dead_code)]
    pub client_os: u16,
    #[allow(dead_code)]
    pub protocol_version: u16,
    #[allow(dead_code)]
    pub xtea_key: Key,
    pub round_keys: RoundKeys,
    pub account: String,
    pub password: String,
    pub character_name: String,
    pub cam_id: u32,
}

pub struct LoginParser;

impl LoginParser {
    pub fn parse(
        raw_packet: &[u8],
        rsa: &RsaDecryptor,
    ) -> Result<LoginInfo, Box<dyn std::error::Error + Send + Sync>> {
        if raw_packet.len() < 128 {
            return Err("Packet too short for login".into());
        }

        // If packet begins with a 4-byte Adler32 checksum, verify and strip it
        let packet = if raw_packet.len() >= 4 + 128 {
            let checksum = u32::from_le_bytes(raw_packet[0..4].try_into().unwrap());
            if adler32(&raw_packet[4..]) == checksum {
                &raw_packet[4..]
            } else {
                raw_packet
            }
        } else {
            raw_packet
        };

        // Try standard RSA block candidate offsets first, then fallback to full scan
        let standard_offsets = [12, 16, 11, 15, 5, 4, 6, 7, 8, 9, 10, 0];
        let mut decrypted_block = None;
        let mut matched_offset = 0;

        for &off in &standard_offsets {
            if off + 128 <= packet.len() {
                if let Some(decrypted) = rsa.decrypt_block(&packet[off..off + 128]) {
                    decrypted_block = Some(decrypted);
                    matched_offset = off;
                    break;
                }
            }
        }

        if decrypted_block.is_none() {
            // Full linear scan in case of custom client padding
            for off in 0..=(packet.len().saturating_sub(128)) {
                if let Some(decrypted) = rsa.decrypt_block(&packet[off..off + 128]) {
                    decrypted_block = Some(decrypted);
                    matched_offset = off;
                    break;
                }
            }
        }

        let decrypted = decrypted_block
            .ok_or_else(|| "RSA decryption failed: invalid key or block structure")?;

        // Extract client_os and protocol_version from header preceding RSA block
        let (client_os, protocol_version) = if matched_offset >= 11 {
            let os = u16::from_le_bytes([packet[matched_offset - 11], packet[matched_offset - 10]]);
            let ver = u16::from_le_bytes([packet[matched_offset - 9], packet[matched_offset - 8]]);
            (os, ver)
        } else if matched_offset >= 4 {
            let os = u16::from_le_bytes([packet[matched_offset - 4], packet[matched_offset - 3]]);
            let ver = u16::from_le_bytes([packet[matched_offset - 2], packet[matched_offset - 1]]);
            (os, ver)
        } else {
            (2, 1098)
        };

        // Format of decrypted block:
        // [0]: 0x00 (already verified by decrypt_block)
        // [1..17]: 16 bytes XTEA key (4 x u32 LE)
        let xtea_key: Key = [
            u32::from_le_bytes(decrypted[1..5].try_into().unwrap()),
            u32::from_le_bytes(decrypted[5..9].try_into().unwrap()),
            u32::from_le_bytes(decrypted[9..13].try_into().unwrap()),
            u32::from_le_bytes(decrypted[13..17].try_into().unwrap()),
        ];
        let round_keys = xtea::expand_key(&xtea_key);

        // Read strings after XTEA key
        let mut cur = Cursor::new(&decrypted[17..]);
        let _gm_flag = cur.read_u8().unwrap_or(0);

        let read_string = |c: &mut Cursor<&[u8]>| -> Option<String> {
            let len = c.read_u16::<LittleEndian>().ok()? as usize;
            let mut buf = vec![0u8; len];
            use std::io::Read;
            c.read_exact(&mut buf).ok()?;
            Some(String::from_utf8_lossy(&buf).to_string())
        };

        let first_str = read_string(&mut cur).unwrap_or_default();
        let second_str = read_string(&mut cur).unwrap_or_default();
        let third_str = read_string(&mut cur).unwrap_or_default();

        let (account, password, character_name) = if first_str.contains('\n') {
            // Standard 10.98 session string: "account\npassword\ntoken\ntime"
            let lines: Vec<&str> = first_str.split('\n').collect();
            let acc = lines.first().copied().unwrap_or("").to_string();
            let pass = lines.get(1).copied().unwrap_or("").to_string();
            let char_name = if !second_str.is_empty() {
                second_str
            } else {
                lines.get(2).copied().unwrap_or("").to_string()
            };
            (acc, pass, char_name)
        } else if !third_str.is_empty() {
            // Old format: login, character, password
            (first_str, third_str, second_str)
        } else {
            // Fallback
            (first_str, "".to_string(), second_str)
        };

        // Extract Cam ID from the suffix after the last ';'.
        // Example: "2026-10-03 13:22:12 Boss; final hunt, 20 RP;1A3" -> 0x1A3 = 419
        let cam_id = Self::parse_cam_id(&character_name)?;

        Ok(LoginInfo {
            client_os,
            protocol_version,
            xtea_key,
            round_keys,
            account,
            password,
            character_name,
            cam_id,
        })
    }

    pub fn parse_cam_id(character_name: &str) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        let trimmed = character_name.trim();
        let hex_str = if let Some(last_semicolon) = trimmed.rfind(';') {
            trimmed[last_semicolon + 1..].trim()
        } else if let Some(last_comma) = trimmed.rfind(',') {
            trimmed[last_comma + 1..].trim()
        } else {
            trimmed
        };

        // Try hex parse first (e.g. "1A3" -> 419, "1" -> 1)
        if let Ok(id) = u32::from_str_radix(hex_str, 16) {
            return Ok(id);
        }

        // Try decimal fallback (e.g. "419")
        if let Ok(id) = hex_str.parse::<u32>() {
            return Ok(id);
        }

        Err(format!("Could not extract Cam ID from character name: '{}'", character_name).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_id_after_semicolon() {
        let name = "2026-11-08 04:00:00 Test Player, 100 EK;1A3";
        assert_eq!(LoginParser::parse_cam_id(name).unwrap(), 0x1A3);
    }

    #[test]
    fn uses_suffix_after_last_semicolon_when_title_contains_separators() {
        let name = "2026-11-08 04:00:00 Boss; final hunt, 100 EK;1A3";
        assert_eq!(LoginParser::parse_cam_id(name).unwrap(), 0x1A3);
    }

    #[test]
    fn still_parses_legacy_comma_suffix() {
        let name = "2026-11-08 04:00:00 Test Player, 100 EK, 1A3";
        assert_eq!(LoginParser::parse_cam_id(name).unwrap(), 0x1A3);
    }

    #[test]
    fn trims_whitespace_after_semicolon() {
        let name = "2026-11-08 04:00:00 Test Player, 100 EK; 1A3";
        assert_eq!(LoginParser::parse_cam_id(name).unwrap(), 0x1A3);
    }
}
