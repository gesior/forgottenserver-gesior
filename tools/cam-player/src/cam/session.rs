use super::parser::{ParsedCam, CAM_PACKET_OUTGOING};
use chrono::{DateTime, TimeZone, Utc};
use std::sync::Arc;
use std::time::Instant;

pub struct PlaybackSession {
    pub cam: Arc<ParsedCam>,
    pub speed: f64,
    pub virtual_time_ms: u64,
    pub current_packet_index: usize,
    pub offset_start_ms: u64,
    pub playback_limit_ms: u64,
    pub is_finished: bool,
    last_tick: Instant,
}

impl PlaybackSession {
    pub fn new(cam: Arc<ParsedCam>, offset_start_sec: i32, offset_end_sec: i32) -> Self {
        let offset_start_ms = (offset_start_sec.max(0) as u64) * 1000;
        let offset_end_ms = (offset_end_sec.max(0) as u64) * 1000;

        let total_duration = cam.duration_ms;
        let playback_limit_ms = total_duration.saturating_sub(offset_end_ms);

        let mut session = Self {
            cam,
            speed: 1.0,
            virtual_time_ms: 0,
            current_packet_index: 0,
            offset_start_ms,
            playback_limit_ms,
            is_finished: false,
            last_tick: Instant::now(),
        };

        // Initialize virtual time at offset_start
        session.virtual_time_ms = session.offset_start_ms;
        session
    }

    /// Fast-seek to a relative millisecond.
    ///
    /// Returns packets that must be sent immediately, in order:
    /// the latest `0x03` snapshot at or before the target (full client state),
    /// then every `0x01` packet after that snapshot and before the target.
    /// Without a snapshot, the catch-up starts at packet 0 so the original login is included.
    /// Non-outgoing packets (`0x02` incoming, `0x04` custom data) and earlier snapshots are not included.
    /// `current_packet_index` is left on the first packet at or after the target.
    pub fn seek_to(&mut self, target_ms: u64) -> Vec<Vec<u8>> {
        let clamped_target = target_ms.min(self.playback_limit_ms).max(self.offset_start_ms);
        self.virtual_time_ms = clamped_target;
        self.last_tick = Instant::now();

        let mut best_snapshot = None;
        for snapshot in &self.cam.sync_snapshots {
            if snapshot.rel_ms <= clamped_target {
                best_snapshot = Some(snapshot);
            } else {
                break;
            }
        }

        let mut burst = Vec::new();

        if let Some(snapshot) = best_snapshot {
            for &idx in &snapshot.packet_indices {
                if let Some(pkt) = self.cam.packets.get(idx) {
                    if !pkt.payload.is_empty() {
                        burst.push(pkt.payload.clone());
                    }
                }
            }

            if let Some(&last_idx) = snapshot.packet_indices.last() {
                self.current_packet_index = last_idx + 1;
            }
        } else {
            self.current_packet_index = 0;
        }

        while self.current_packet_index < self.cam.packets.len() {
            let pkt = &self.cam.packets[self.current_packet_index];
            if pkt.rel_ms >= self.virtual_time_ms {
                break;
            }
            if pkt.direction == CAM_PACKET_OUTGOING && !pkt.payload.is_empty() {
                burst.push(pkt.payload.clone());
            }
            self.current_packet_index += 1;
        }

        burst
    }

    /// Advances virtual playback clock and retrieves all 0x01 game packets that are due.
    /// Returns (packets_to_send, reached_end)
    pub fn step(&mut self) -> (Vec<Vec<u8>>, bool) {
        if self.is_finished {
            return (Vec::new(), true);
        }

        let now = Instant::now();
        let dt = now.duration_since(self.last_tick);
        self.last_tick = now;

        let delta_virtual_ms = (dt.as_secs_f64() * 1000.0 * self.speed) as u64;
        self.virtual_time_ms = self.virtual_time_ms.saturating_add(delta_virtual_ms);

        // Check offset_end cut-off
        if self.virtual_time_ms >= self.playback_limit_ms {
            self.is_finished = true;
            return (Vec::new(), true);
        }

        let mut packets_to_send = Vec::new();

        while self.current_packet_index < self.cam.packets.len() {
            let pkt = &self.cam.packets[self.current_packet_index];
            if pkt.rel_ms > self.virtual_time_ms {
                break;
            }

            // Only stream 0x01 game packets in regular playback
            if pkt.direction == CAM_PACKET_OUTGOING && !pkt.payload.is_empty() {
                packets_to_send.push(pkt.payload.clone());
            }

            self.current_packet_index += 1;
        }

        if self.current_packet_index >= self.cam.packets.len() {
            self.is_finished = true;
            return (packets_to_send, true);
        }

        (packets_to_send, false)
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.125, 64.0);
    }

    /// Playback window after trimming the hidden start and end offsets.
    pub fn visible_duration_ms(&self) -> u64 {
        self.playback_limit_ms.saturating_sub(self.offset_start_ms)
    }

    /// `HH:MM:SS` label of [`Self::visible_duration_ms`].
    pub fn visible_duration_label(&self) -> String {
        format_hms(self.visible_duration_ms())
    }

    /// Bottom-bar status: server-local time of the current frame, trimmed progress, and speed.
    pub fn status_line(&self) -> String {
        let unix_ms = self
            .cam
            .first_packet_time_ms
            .saturating_add(self.virtual_time_ms);
        let elapsed_ms = self.virtual_time_ms.saturating_sub(self.offset_start_ms);
        let total_ms = self.visible_duration_ms();
        format_playback_status(unix_ms, elapsed_ms, total_ms, self.speed, &chrono::Local)
    }
}

fn format_playback_status<Tz>(
    unix_ms: u64,
    elapsed_ms: u64,
    total_ms: u64,
    speed: f64,
    time_zone: &Tz,
) -> String
where
    Tz: TimeZone,
    Tz::Offset: std::fmt::Display,
{
    format!(
        "{} {}/{} Speed: {}",
        format_wall_clock(unix_ms, time_zone),
        format_hms(elapsed_ms),
        format_hms(total_ms),
        format_speed(speed)
    )
}

fn format_wall_clock<Tz>(unix_ms: u64, time_zone: &Tz) -> String
where
    Tz: TimeZone,
    Tz::Offset: std::fmt::Display,
{
    let secs = i64::try_from(unix_ms / 1000).unwrap_or(i64::MAX);
    let utc = DateTime::<Utc>::from_timestamp(secs, 0).unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
    utc.with_timezone(time_zone)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn format_hms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn format_speed(speed: f64) -> String {
    if !speed.is_finite() {
        return "0x".to_string();
    }

    let rounded = (speed * 1000.0).round() / 1000.0;
    let nearest = rounded.round();
    if (rounded - nearest).abs() < 1e-9 {
        return format!("{nearest:.0}x");
    }

    let text = format!("{rounded:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}x")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cam::parser::{
        CamPacket, SyncSnapshot, CAM_PACKET_CUSTOM_DATA, CAM_PACKET_INCOMING, CAM_PACKET_OUTGOING,
        CAM_PACKET_SYNCHRONIZE,
    };
    use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
    use std::sync::Arc;

    fn utc_ms(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> u64 {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(hour, min, sec)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64
    }

    fn test_cam(first_packet_time_ms: u64, duration_ms: u64) -> Arc<ParsedCam> {
        Arc::new(ParsedCam {
            player_id: 1,
            character_name: "Tester".to_string(),
            protocol_version: 1098,
            start_time_ms: first_packet_time_ms as i64,
            first_packet_time_ms,
            last_packet_time_ms: first_packet_time_ms + duration_ms,
            duration_ms,
            packets: Vec::new(),
            sync_snapshots: Vec::new(),
        })
    }

    #[test]
    fn status_line_converts_utc_to_the_given_zone() {
        let unix_ms = utc_ms(2026, 11, 9, 14, 4, 15) + 900;
        let zone = FixedOffset::east_opt(3600).unwrap();
        let line = format_playback_status(unix_ms, 15_000, 60_000, 2.0, &zone);
        assert_eq!(line, "2026-11-09 15:04:15 00:00:15/00:01:00 Speed: 2x");
    }

    #[test]
    fn status_line_floors_subsecond_progress_and_formats_speed() {
        let zone = FixedOffset::east_opt(0).unwrap();
        let unix_ms = utc_ms(2026, 11, 9, 0, 0, 0);
        let line = format_playback_status(unix_ms, 15_900, 60_400, 0.5, &zone);
        assert_eq!(line, "2026-11-09 00:00:00 00:00:15/00:01:00 Speed: 0.5x");

        assert!(format_playback_status(unix_ms, 0, 0, 1.0, &zone).ends_with("Speed: 1x"));
        assert!(format_playback_status(unix_ms, 0, 0, 0.125, &zone).ends_with("Speed: 0.125x"));
        assert!(format_playback_status(unix_ms, 0, 0, 1.5, &zone).ends_with("Speed: 1.5x"));
    }

    #[test]
    fn status_line_hours_are_not_wrapped_after_24() {
        let zone = FixedOffset::east_opt(0).unwrap();
        let day = 26 * 3600 * 1000;
        let line = format_playback_status(0, day, day, 1.0, &zone);
        assert!(line.contains("26:00:00/26:00:00 Speed: 1x"));
    }

    #[test]
    fn session_status_uses_offsets_and_server_local_time() {
        let first = utc_ms(2026, 11, 9, 14, 3, 30);
        let mut session = PlaybackSession::new(test_cam(first, 100_000), 30, 10);
        session.virtual_time_ms = 45_000;
        session.speed = 2.0;

        let played_secs = i64::try_from((first + 45_000) / 1000).unwrap();
        let wall = DateTime::<Utc>::from_timestamp(played_secs, 0)
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S");
        assert_eq!(
            session.status_line(),
            format!("{wall} 00:00:15/00:01:00 Speed: 2x")
        );

        let trimmed_end = PlaybackSession::new(test_cam(first, 100_000), 0, 10);
        assert!(trimmed_end.status_line().contains("00:00:00/00:01:30 Speed: 1x"));
    }

    #[test]
    fn visible_duration_hides_start_and_end_offsets() {
        let session = PlaybackSession::new(test_cam(0, 100_000), 30, 10);
        assert_eq!(session.visible_duration_ms(), 60_000);
        assert_eq!(session.visible_duration_label(), "00:01:00");
    }

    fn packet(rel_ms: u64, direction: u8, payload: &[u8]) -> CamPacket {
        CamPacket {
            rel_ms,
            direction,
            payload: payload.to_vec(),
        }
    }

    fn seek_cam(packets: Vec<CamPacket>, sync_snapshots: Vec<SyncSnapshot>) -> Arc<ParsedCam> {
        Arc::new(ParsedCam {
            player_id: 1,
            character_name: "Tester".to_string(),
            protocol_version: 1098,
            start_time_ms: 0,
            first_packet_time_ms: 0,
            last_packet_time_ms: 20_000,
            duration_ms: 20_000,
            packets,
            sync_snapshots,
        })
    }

    #[test]
    fn seek_sends_snapshot_then_catchup_packets_before_the_target() {
        let cam = seek_cam(
            vec![
                packet(0, CAM_PACKET_OUTGOING, b"login"),
                packet(5_000, CAM_PACKET_SYNCHRONIZE, b"snap-a"),
                packet(5_000, CAM_PACKET_SYNCHRONIZE, b"snap-b"),
                packet(5_200, CAM_PACKET_INCOMING, b"client-input"),
                packet(5_300, CAM_PACKET_CUSTOM_DATA, b"custom-data"),
                packet(5_500, CAM_PACKET_OUTGOING, b"delta"),
                packet(7_000, CAM_PACKET_OUTGOING, b"later"),
            ],
            vec![SyncSnapshot {
                rel_ms: 5_000,
                packet_indices: vec![1, 2],
            }],
        );
        let mut session = PlaybackSession::new(cam, 0, 0);

        let burst = session.seek_to(6_000);

        assert_eq!(
            burst,
            vec![b"snap-a".to_vec(), b"snap-b".to_vec(), b"delta".to_vec()]
        );
        assert_eq!(session.current_packet_index, 6);
        assert_eq!(session.cam.packets[session.current_packet_index].payload, b"later");
    }

    #[test]
    fn seek_to_zero_sends_nothing_and_stays_at_the_first_packet() {
        let cam = seek_cam(
            vec![
                packet(0, CAM_PACKET_OUTGOING, b"login"),
                packet(5_000, CAM_PACKET_SYNCHRONIZE, b"snap"),
                packet(5_500, CAM_PACKET_OUTGOING, b"delta"),
            ],
            vec![SyncSnapshot {
                rel_ms: 5_000,
                packet_indices: vec![1],
            }],
        );
        let mut session = PlaybackSession::new(cam, 0, 0);

        let burst = session.seek_to(0);

        assert!(burst.is_empty());
        assert_eq!(session.current_packet_index, 0);
    }

    #[test]
    fn seek_before_the_first_snapshot_replays_game_packets_from_the_start() {
        let cam = seek_cam(
            vec![
                packet(0, CAM_PACKET_OUTGOING, b"login"),
                packet(1_000, CAM_PACKET_INCOMING, b"client-input"),
                packet(1_500, CAM_PACKET_CUSTOM_DATA, b"custom-data"),
                packet(2_000, CAM_PACKET_OUTGOING, b"early"),
                packet(4_000, CAM_PACKET_OUTGOING, b"after-target"),
                packet(5_000, CAM_PACKET_SYNCHRONIZE, b"snap"),
            ],
            vec![SyncSnapshot {
                rel_ms: 5_000,
                packet_indices: vec![5],
            }],
        );
        let mut session = PlaybackSession::new(cam, 0, 0);

        let burst = session.seek_to(3_000);

        assert_eq!(burst, vec![b"login".to_vec(), b"early".to_vec()]);
        assert_eq!(session.current_packet_index, 4);
        assert!(burst.iter().all(|payload| payload != b"snap"));
    }
}
