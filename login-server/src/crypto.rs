// Copyright 2026 The Forgotten Server Authors. All rights reserved.
// Use of this source code is governed by the GPL-2.0 License that can be found in the LICENSE file.

use std::path::Path;

use anyhow::Context;
use hmac::{Hmac, Mac};
use openssl::pkey::Private;
use openssl::rsa::{Padding, Rsa};
use sha1::{Digest, Sha1};

pub const RSA_BLOCK: usize = 128;

pub type RoundKeys = [u32; 64];

type HmacSha1 = Hmac<Sha1>;

pub struct RsaKey {
    inner: Rsa<Private>,
}

impl RsaKey {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let pem = std::fs::read(path)
            .with_context(|| format!("failed to read RSA key {}", path.display()))?;
        Self::from_pem(&pem)
    }

    pub fn from_pem(pem: &[u8]) -> anyhow::Result<Self> {
        let inner = Rsa::private_key_from_pem(pem).context("failed to parse RSA private key")?;
        if inner.size() as usize != RSA_BLOCK {
            anyhow::bail!(
                "RSA key must be 1024-bit so login blocks stay {RSA_BLOCK} bytes, got {} bytes",
                inner.size()
            );
        }
        Ok(Self { inner })
    }

    /// Raw RSA private operation (`RSA_NO_PADDING`), same as `tfs::rsa::decrypt`.
    /// Leading zeros are restored so the Tibia plaintext still starts with `0x00`.
    pub fn decrypt_block(&self, input: &[u8]) -> Option<[u8; RSA_BLOCK]> {
        if input.len() != RSA_BLOCK {
            return None;
        }
        let mut buf = [0u8; RSA_BLOCK];
        let written = self
            .inner
            .private_decrypt(input, &mut buf, Padding::NONE)
            .ok()?;
        if written == 0 || written > RSA_BLOCK {
            return None;
        }
        let mut out = [0u8; RSA_BLOCK];
        out[RSA_BLOCK - written..].copy_from_slice(&buf[..written]);
        if out[0] != 0 {
            return None;
        }
        Some(out)
    }

    #[cfg(test)]
    pub fn encrypt_block(&self, plain: &[u8; RSA_BLOCK]) -> anyhow::Result<[u8; RSA_BLOCK]> {
        let mut buf = [0u8; RSA_BLOCK];
        let written = self
            .inner
            .public_encrypt(plain, &mut buf, Padding::NONE)
            .context("RSA public encrypt")?;
        if written == 0 || written > RSA_BLOCK {
            anyhow::bail!("unexpected RSA ciphertext length {written}");
        }
        let mut out = [0u8; RSA_BLOCK];
        out[RSA_BLOCK - written..].copy_from_slice(&buf[..written]);
        Ok(out)
    }
}

pub fn expand_key(key: [u32; 4]) -> RoundKeys {
    const DELTA: u32 = 0x9E3779B9;
    let mut expanded = [0u32; 64];
    let mut sum = 0u32;
    let mut next_sum = sum.wrapping_add(DELTA);
    let mut index = 0;
    while index < expanded.len() {
        expanded[index] = sum.wrapping_add(key[(sum & 3) as usize]);
        expanded[index + 1] = next_sum.wrapping_add(key[((next_sum >> 11) & 3) as usize]);
        index += 2;
        sum = next_sum;
        next_sum = next_sum.wrapping_add(DELTA);
    }
    expanded
}

/// TFS encrypts every block for one round before moving to the next round.
pub fn xtea_encrypt(data: &mut [u8], key: &RoundKeys) {
    if data.len() % 8 != 0 {
        return;
    }
    let mut round = 0;
    while round < key.len() {
        let mut offset = 0;
        while offset < data.len() {
            let mut left = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
            let mut right = u32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap());
            left = left.wrapping_add(((right << 4) ^ (right >> 5)).wrapping_add(right) ^ key[round]);
            right = right.wrapping_add(((left << 4) ^ (left >> 5)).wrapping_add(left) ^ key[round + 1]);
            data[offset..offset + 4].copy_from_slice(&left.to_le_bytes());
            data[offset + 4..offset + 8].copy_from_slice(&right.to_le_bytes());
            offset += 8;
        }
        round += 2;
    }
}

/// Inverse of `xtea_encrypt`. The login server only encrypts replies; decrypt is covered by tests.
#[cfg_attr(not(test), allow(dead_code))]
pub fn xtea_decrypt(data: &mut [u8], key: &RoundKeys) {
    if data.len() % 8 != 0 {
        return;
    }
    let mut round = key.len();
    while round >= 2 {
        round -= 2;
        let mut offset = 0;
        while offset < data.len() {
            let mut left = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
            let mut right = u32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap());
            right = right.wrapping_sub(((left << 4) ^ (left >> 5)).wrapping_add(left) ^ key[round + 1]);
            left = left.wrapping_sub(((right << 4) ^ (right >> 5)).wrapping_add(right) ^ key[round]);
            data[offset..offset + 4].copy_from_slice(&left.to_le_bytes());
            data[offset + 4..offset + 8].copy_from_slice(&right.to_le_bytes());
            offset += 8;
        }
    }
}

/// Adler-32 as implemented by TFS (`adlerChecksum`). Returns 0 for oversized buffers.
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    const MAX: usize = 24590;
    if data.len() > MAX {
        return 0;
    }
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    let mut rest = data;
    while !rest.is_empty() {
        let take = rest.len().min(5552);
        let (chunk, next) = rest.split_at(take);
        rest = next;
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

pub fn sha1_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha1::digest(data);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Base32 used by TFS `decodeSecret`. Invalid alphabets yield an empty key, which disables 2FA.
pub fn decode_secret(secret: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(10);
    let mut buffer: u32 = 0;
    let mut left: u32 = 0;
    for &ch in secret.as_bytes() {
        buffer = buffer.wrapping_shl(5);
        let value = if (b'A'..=b'Z').contains(&ch) {
            u32::from((ch & 0x1f) - 1)
        } else if (b'2'..=b'7').contains(&ch) {
            u32::from(ch - 24)
        } else {
            return Vec::new();
        };
        buffer |= value;
        left += 5;
        if left >= 8 {
            left -= 8;
            key.push((buffer >> left) as u8);
        }
    }
    key
}

pub fn hotp(secret: &[u8], counter: u64) -> Option<String> {
    let mut mac = HmacSha1::new_from_slice(secret).ok()?;
    mac.update(&counter.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = (hash[19] & 0x0f) as usize;
    let binary = u32::from_be_bytes([
        hash[offset],
        hash[offset + 1],
        hash[offset + 2],
        hash[offset + 3],
    ]) & 0x7fff_ffff;
    Some(format!("{:06}", binary % 1_000_000))
}

pub fn token_matches(secret: &[u8], token: &[u8], ticks: u32) -> bool {
    let Ok(token) = std::str::from_utf8(token) else {
        return false;
    };
    [ticks, ticks.wrapping_sub(1), ticks.wrapping_add(1)]
        .into_iter()
        .any(|counter| hotp(secret, u64::from(counter)).as_deref() == Some(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_known_vectors() {
        assert_eq!(
            sha1_hex(b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            sha1_hex(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn adler_matches_tfs_short_inputs() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"a"), 0x0062_0062);
        assert_eq!(adler32(&[0u8; 24591]), 0);
    }

    #[test]
    fn decode_secret_matches_tfs_base32() {
        assert_eq!(decode_secret("AAAAAAAAAAAAAAAA"), vec![0u8; 10]);
        assert!(decode_secret("AAAAAAAAAAAAAAAa").is_empty());
        assert!(decode_secret("AAAA====AAAAAAAA").is_empty());
        assert!(decode_secret("").is_empty());
    }

    #[test]
    fn hotp_matches_rfc6238_sha1() {
        let secret = b"12345678901234567890";
        assert_eq!(hotp(secret, 1).as_deref(), Some("287082"));
        assert_eq!(hotp(secret, 1111111109 / 30).as_deref(), Some("081804"));
        assert_eq!(hotp(secret, 2000000000 / 30).as_deref(), Some("279037"));
        assert!(token_matches(secret, b"287082", 1));
        assert!(token_matches(secret, b"287082", 2));
        assert!(!token_matches(secret, b"000000", 1));
    }

    #[test]
    fn xtea_matches_tfs_vectors_and_roundtrips() {
        let key = expand_key([0x1111_1111, 0x2222_2222, 0x3333_3333, 0x4444_4444]);
        let mut block = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        xtea_encrypt(&mut block, &key);
        assert_eq!(block, [0x0a, 0x9a, 0xd1, 0xd9, 0x27, 0x12, 0xa1, 0x9c]);
        xtea_decrypt(&mut block, &key);
        assert_eq!(block, [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);

        let key = expand_key([1, 2, 3, 4]);
        let mut blocks = [
            0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
        ];
        xtea_encrypt(&mut blocks, &key);
        assert_eq!(
            blocks,
            [
                0x23, 0x66, 0x94, 0xc1, 0x3b, 0xbb, 0xb7, 0xbd, 0x4b, 0xd4, 0x2c, 0x39, 0x11, 0x8c, 0xd9, 0xfd,
            ]
        );
        xtea_decrypt(&mut blocks, &key);
        assert_eq!(
            blocks,
            [0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,]
        );
    }

    #[test]
    fn rsa_no_padding_roundtrip_preserves_leading_zero() {
        let key = RsaKey::from_pem(include_bytes!("../tests/fixtures/key.pem")).unwrap();
        let mut plain = [0u8; RSA_BLOCK];
        for (index, byte) in plain.iter_mut().enumerate().skip(1) {
            *byte = index as u8;
        }
        let cipher = key.encrypt_block(&plain).unwrap();
        let decoded = key.decrypt_block(&cipher).unwrap();
        assert_eq!(decoded, plain);
    }
}
