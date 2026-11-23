use byteorder::{LittleEndian, WriteBytesExt};

/// Blue text in the console (`MESSAGE_STATUS_CONSOLE_BLUE`).
pub const MESSAGE_STATUS_CONSOLE_BLUE: u8 = 4;

/// White text at the bottom of the game window (`MESSAGE_STATUS_SMALL`).
pub const MESSAGE_STATUS_SMALL: u8 = 21;

pub struct MessageBuilder;

impl MessageBuilder {
    /// Builds a Tibia 0xB4 text message (console, center red message, status).
    pub fn text_message(text: &str, message_type: u8) -> Vec<u8> {
        let bytes = text.as_bytes();
        let len = bytes.len() as u16;

        let mut msg = Vec::with_capacity(4 + bytes.len());
        msg.push(0xB4);
        msg.push(message_type);
        msg.write_u16::<LittleEndian>(len).unwrap();
        msg.extend_from_slice(bytes);
        msg
    }

    /// Builds a Tibia 0x14 error / disconnect message.
    pub fn error_message(text: &str) -> Vec<u8> {
        let bytes = text.as_bytes();
        let len = bytes.len() as u16;

        let mut msg = Vec::with_capacity(3 + bytes.len());
        msg.push(0x14);
        msg.write_u16::<LittleEndian>(len).unwrap();
        msg.extend_from_slice(bytes);
        msg
    }

    /// Cancel walk packet (0xB5 0x02) to keep viewer character from moving.
    pub fn cancel_walk() -> Vec<u8> {
        vec![0xB5, 0x02]
    }

    /// Keepalive Pong response (0x1E).
    pub fn pong() -> Vec<u8> {
        vec![0x1E]
    }
}
