use crate::crypto::adler32::adler32;
use byteorder::{LittleEndian, WriteBytesExt};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ChallengePacket {
    pub raw_bytes: Vec<u8>,
}

impl ChallengePacket {
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;

        let random: u8 = ((timestamp ^ (timestamp >> 8) ^ 0x5A) & 0xFF) as u8;

        // Challenge payload:
        // [0..1]: uint16 payload length = 6 (0x0006)
        // [2]: uint8 opcode = 0x1F
        // [3..6]: uint32 timestamp
        // [7]: uint8 random
        let mut payload = Vec::with_capacity(8);
        payload.write_u16::<LittleEndian>(6).unwrap();
        payload.push(0x1F);
        payload.write_u32::<LittleEndian>(timestamp).unwrap();
        payload.push(random);

        let checksum = adler32(&payload);

        // Outer packet:
        // [0..1]: uint16 total length = 4 (checksum) + 8 (payload) = 12 (0x000C)
        // [2..5]: uint32 checksum
        // [6..13]: payload (8 bytes)
        let mut raw = Vec::with_capacity(14);
        raw.write_u16::<LittleEndian>(12).unwrap();
        raw.write_u32::<LittleEndian>(checksum).unwrap();
        raw.extend_from_slice(&payload);

        Self { raw_bytes: raw }
    }
}
