use crate::crypto::adler32::adler32;
use crate::crypto::xtea::{self, RoundKeys};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

pub struct PacketFramer;

impl PacketFramer {
    /// Packs one or more concatenated game messages into an XTEA-encrypted, Adler-checksummed Tibia packet.
    pub fn pack_outgoing(payload: &[u8], round_keys: &RoundKeys) -> Vec<u8> {
        let inner_len = payload.len();
        // 2 bytes for inner length header + payload
        let unpadded_len = 2 + inner_len;
        let pad_bytes = if unpadded_len % 8 != 0 {
            8 - (unpadded_len % 8)
        } else {
            0
        };
        let encrypted_block_len = unpadded_len + pad_bytes;

        let mut block = Vec::with_capacity(encrypted_block_len);
        block.write_u16::<LittleEndian>(inner_len as u16).unwrap();
        block.extend_from_slice(payload);
        block.resize(encrypted_block_len, 0u8);

        // XTEA in-place encryption
        xtea::encrypt(&mut block, round_keys);

        // Adler32 checksum over the encrypted block
        let checksum = adler32(&block);

        // Outer packet: 2 bytes outer_len + 4 bytes checksum + encrypted_block
        let outer_len = (4 + encrypted_block_len) as u16;
        let mut packet = Vec::with_capacity(2 + 4 + encrypted_block_len);
        packet.write_u16::<LittleEndian>(outer_len).unwrap();
        packet.write_u32::<LittleEndian>(checksum).unwrap();
        packet.extend_from_slice(&block);

        packet
    }

    /// Unpacks an incoming XTEA-encrypted packet (after the 2-byte outer size header).
    pub fn unpack_incoming(
        raw_body: &[u8],
        round_keys: &RoundKeys,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        if raw_body.len() < 12 {
            return Err("Packet too short for XTEA body".into());
        }

        let mut cur = Cursor::new(&raw_body[0..4]);
        let checksum = cur.read_u32::<LittleEndian>()?;
        let encrypted_block = &raw_body[4..];

        if encrypted_block.len() % 8 != 0 {
            return Err("Encrypted block not a multiple of 8".into());
        }

        let actual_checksum = adler32(encrypted_block);
        if actual_checksum != checksum {
            return Err("Adler32 checksum mismatch".into());
        }

        let mut decrypted = encrypted_block.to_vec();
        xtea::decrypt(&mut decrypted, round_keys);

        let mut dec_cur = Cursor::new(&decrypted[0..2]);
        let inner_len = dec_cur.read_u16::<LittleEndian>()? as usize;

        if 2 + inner_len > decrypted.len() {
            return Err("Inner length exceeds decrypted payload".into());
        }

        Ok(decrypted[2..2 + inner_len].to_vec())
    }

    /// Sends a packed packet over an async TCP stream.
    pub async fn send_packet(
        writer: &mut OwnedWriteHalf,
        payload: &[u8],
        round_keys: &RoundKeys,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use tokio::io::AsyncWriteExt;
        let packet = Self::pack_outgoing(payload, round_keys);
        writer.write_all(&packet).await?;
        writer.flush().await?;
        Ok(())
    }

    /// Reads raw outer packet (2-byte length + body) without decrypting.
    pub async fn read_raw_packet(
        reader: &mut OwnedReadHalf,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        use tokio::io::AsyncReadExt;
        let mut len_buf = [0u8; 2];
        reader.read_exact(&mut len_buf).await?;
        let packet_len = u16::from_le_bytes(len_buf) as usize;

        let mut body = vec![0u8; packet_len];
        reader.read_exact(&mut body).await?;
        Ok(body)
    }

    /// Reads and decrypts an incoming XTEA packet.
    pub async fn read_decrypted_packet(
        reader: &mut OwnedReadHalf,
        round_keys: &RoundKeys,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let body = Self::read_raw_packet(reader).await?;
        Self::unpack_incoming(&body, round_keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_framer_roundtrip() {
        let key: xtea::Key = [0x11111111, 0x22222222, 0x33333333, 0x44444444];
        let rk = xtea::expand_key(&key);
        let sample_game_payload = b"Example Game Chat Message /goto 50";

        let packed = PacketFramer::pack_outgoing(sample_game_payload, &rk);
        assert!(packed.len() >= 2 + 4 + 8);

        // Body starts after 2-byte outer size
        let body = &packed[2..];
        let unpacked = PacketFramer::unpack_incoming(body, &rk).unwrap();
        assert_eq!(unpacked, sample_game_payload);
    }
}
