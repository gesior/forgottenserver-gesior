use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read};

pub const CAM_PACKET_OUTGOING: u8 = 0x01;
#[allow(dead_code)]
pub const CAM_PACKET_INCOMING: u8 = 0x02;
pub const CAM_PACKET_SYNCHRONIZE: u8 = 0x03;
#[allow(dead_code)]
pub const CAM_PACKET_CUSTOM_DATA: u8 = 0x04;

#[derive(Debug, Clone)]
pub struct CamPacket {
    pub rel_ms: u64,
    pub direction: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct SyncSnapshot {
    pub rel_ms: u64,
    pub packet_indices: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct ParsedCam {
    #[allow(dead_code)]
    pub player_id: u32,
    pub character_name: String,
    #[allow(dead_code)]
    pub protocol_version: u16,
    #[allow(dead_code)]
    pub start_time_ms: i64,
    pub first_packet_time_ms: u64,
    #[allow(dead_code)]
    pub last_packet_time_ms: u64,
    pub duration_ms: u64,
    pub packets: Vec<CamPacket>,
    pub sync_snapshots: Vec<SyncSnapshot>,
}

pub struct CamParser;

impl CamParser {
    pub fn parse(bytes: &[u8]) -> Result<ParsedCam, Box<dyn std::error::Error + Send + Sync>> {
        if bytes.len() < 8 || &bytes[0..8] != b"TIBIACAM" {
            return Err("Invalid TIBIACAM header magic".into());
        }

        let mut cursor = Cursor::new(&bytes[8..]);
        let version = cursor.read_u8()?;
        if version != 0x01 {
            return Err(format!("Unsupported cam format version: {}", version).into());
        }

        let player_id = cursor.read_u32::<LittleEndian>()?;
        let name_length = cursor.read_u16::<LittleEndian>()? as usize;

        let mut name_buf = vec![0u8; name_length];
        cursor.read_exact(&mut name_buf)?;
        let character_name = String::from_utf8_lossy(&name_buf).to_string();

        let protocol_version = cursor.read_u16::<LittleEndian>()?;
        let start_time_ms = cursor.read_i64::<LittleEndian>()?;

        let mut packets = Vec::new();
        let mut sync_snapshots: Vec<SyncSnapshot> = Vec::new();
        let mut first_time: Option<u64> = None;
        let mut last_time: u64 = 0;

        let mut pos = 8 + 1 + 4 + 2 + name_length + 2 + 8;

        while pos + 10 <= bytes.len() {
            let mut chunk = Cursor::new(&bytes[pos..pos + 10]);
            let timestamp_ms = chunk.read_u64::<LittleEndian>()?;
            let packet_size = chunk.read_u16::<LittleEndian>()? as usize;
            pos += 10;

            if pos + packet_size > bytes.len() {
                // Incomplete or truncated packet at EOF
                break;
            }

            let packet_bytes = &bytes[pos..pos + packet_size];
            pos += packet_size;

            if packet_bytes.is_empty() {
                continue;
            }

            let direction = packet_bytes[0];
            // Only outgoing (0x01) and synchronize (0x03) packets are used for playback;
            // skip incoming (0x02), custom data (0x04), and any unknown packet types.
            if direction != CAM_PACKET_OUTGOING && direction != CAM_PACKET_SYNCHRONIZE {
                continue;
            }

            let payload = packet_bytes[1..].to_vec();

            let first_t = *first_time.get_or_insert(timestamp_ms);
            let rel_ms = timestamp_ms.saturating_sub(first_t);
            last_time = timestamp_ms;

            let idx = packets.len();
            if direction == CAM_PACKET_SYNCHRONIZE {
                let need_new = match sync_snapshots.last() {
                    Some(s) => s.rel_ms != rel_ms,
                    None => true,
                };
                if need_new {
                    sync_snapshots.push(SyncSnapshot {
                        rel_ms,
                        packet_indices: vec![idx],
                    });
                } else {
                    sync_snapshots.last_mut().unwrap().packet_indices.push(idx);
                }
            }

            packets.push(CamPacket {
                rel_ms,
                direction,
                payload,
            });
        }

        let first_packet_time_ms = first_time.unwrap_or(0);
        let last_packet_time_ms = last_time;
        let duration_ms = last_packet_time_ms.saturating_sub(first_packet_time_ms);

        Ok(ParsedCam {
            player_id,
            character_name,
            protocol_version,
            start_time_ms,
            first_packet_time_ms,
            last_packet_time_ms,
            duration_ms,
            packets,
            sync_snapshots,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_cam_bytes(packets: &[(u64, u8, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"TIBIACAM");
        out.push(0x01); // format version
        out.extend_from_slice(&123u32.to_le_bytes()); // player_id
        let name = b"Tester";
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&1098u16.to_le_bytes()); // protocol_version
        out.extend_from_slice(&1_000i64.to_le_bytes()); // start_time_ms

        for (ts, pkt_type, payload) in packets {
            let total_size = (1 + payload.len()) as u16;
            out.extend_from_slice(&ts.to_le_bytes());
            out.extend_from_slice(&total_size.to_le_bytes());
            out.push(*pkt_type);
            out.extend_from_slice(payload);
        }
        out
    }

    #[test]
    fn parse_keeps_outgoing_and_sync_and_skips_unused_types() {
        let raw = build_cam_bytes(&[
            (1_000, CAM_PACKET_INCOMING, b"early-in"),
            (1_500, CAM_PACKET_OUTGOING, b"login"),
            (2_000, CAM_PACKET_INCOMING, b"client-in"),
            (2_500, CAM_PACKET_CUSTOM_DATA, b"anticheat"),
            (3_000, 0x05, b"future-unused"),
            (4_000, CAM_PACKET_SYNCHRONIZE, b"sync-a"),
            (4_000, CAM_PACKET_SYNCHRONIZE, b"sync-b"),
            (5_000, CAM_PACKET_OUTGOING, b"move"),
            (6_000, CAM_PACKET_CUSTOM_DATA, b"trailing-custom"),
        ]);

        let parsed = CamParser::parse(&raw).expect("valid cam");
        assert_eq!(parsed.first_packet_time_ms, 1_500);
        assert_eq!(parsed.last_packet_time_ms, 5_000);
        assert_eq!(parsed.duration_ms, 3_500);

        assert_eq!(parsed.packets.len(), 4);
        assert_eq!(parsed.packets[0].direction, CAM_PACKET_OUTGOING);
        assert_eq!(parsed.packets[0].payload, b"login");
        assert_eq!(parsed.packets[1].direction, CAM_PACKET_SYNCHRONIZE);
        assert_eq!(parsed.packets[1].payload, b"sync-a");
        assert_eq!(parsed.packets[2].direction, CAM_PACKET_SYNCHRONIZE);
        assert_eq!(parsed.packets[2].payload, b"sync-b");
        assert_eq!(parsed.packets[3].direction, CAM_PACKET_OUTGOING);
        assert_eq!(parsed.packets[3].payload, b"move");

        assert_eq!(parsed.sync_snapshots.len(), 1);
        assert_eq!(parsed.sync_snapshots[0].rel_ms, 2_500);
        assert_eq!(parsed.sync_snapshots[0].packet_indices, vec![1, 2]);
    }
}
