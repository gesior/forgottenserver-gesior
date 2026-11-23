use pkcs1::DecodeRsaPrivateKey;
use rsa::traits::{PrivateKeyParts, PublicKeyParts};
use rsa::{BigUint, RsaPrivateKey};
use std::fs;
use std::path::Path;

#[derive(Clone)]
pub struct RsaDecryptor {
    n: BigUint,
    d: BigUint,
}

impl RsaDecryptor {
    pub fn from_pem_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let content = fs::read_to_string(path)?;
        Self::from_pem(&content)
    }

    pub fn from_pem(pem_str: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Extract base64 payload lines, stripping headers and all whitespace
        let b64_payload: String = pem_str
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.starts_with("-----") && !l.is_empty())
            .collect();

        // Reformat to strictly 64 characters per line for strict RFC 7468 PEM parser compliance
        let mut strictly_formatted = String::from("-----BEGIN RSA PRIVATE KEY-----\n");
        for chunk in b64_payload.as_bytes().chunks(64) {
            strictly_formatted.push_str(std::str::from_utf8(chunk)?);
            strictly_formatted.push('\n');
        }
        strictly_formatted.push_str("-----END RSA PRIVATE KEY-----\n");

        let priv_key = RsaPrivateKey::from_pkcs1_pem(&strictly_formatted)?;
        let n = priv_key.n().clone();
        let d = priv_key.d().clone();
        Ok(Self { n, d })
    }

    /// Performs raw RSA decryption (m = c^d mod n) without padding,
    /// formatted to 128 bytes with leading zeros.
    /// Returns None if the block size != 128 or decrypted byte 0 != 0x00.
    pub fn decrypt_block(&self, block: &[u8]) -> Option<[u8; 128]> {
        if block.len() != 128 {
            return None;
        }

        let c = BigUint::from_bytes_be(block);
        let m = c.modpow(&self.d, &self.n);
        let m_bytes = m.to_bytes_be();

        if m_bytes.len() > 128 {
            return None;
        }

        let mut out = [0u8; 128];
        let offset = 128 - m_bytes.len();
        out[offset..].copy_from_slice(&m_bytes);

        // Tibia protocol verification: first byte must be 0x00
        if out[0] != 0x00 {
            return None;
        }

        Some(out)
    }
}
