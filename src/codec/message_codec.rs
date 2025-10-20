//! Codec for framing and encoding/decoding gossip messages.

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::{Error, Message, Result};

const MAX_FRAME_SIZE: usize = 10 * 1024 * 1024; // 10 MB

/// Codec for gossip messages.
///
/// Uses length-prefixed framing: [4 bytes length][message bytes]
#[derive(Debug, Clone)]
pub struct MessageCodec {
    max_frame_size: usize,
}

impl MessageCodec {
    /// Create a new codec with default max frame size.
    pub fn new() -> Self {
        Self {
            max_frame_size: MAX_FRAME_SIZE,
        }
    }

    /// Create a new codec with custom max frame size.
    pub fn with_max_frame_size(max_frame_size: usize) -> Self {
        Self { max_frame_size }
    }
}

impl Default for MessageCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>> {
        if src.len() < 4 {
            return Ok(None);
        }

        let mut length_bytes = [0u8; 4];
        length_bytes.copy_from_slice(&src[..4]);
        let length = u32::from_be_bytes(length_bytes) as usize;

        if length > self.max_frame_size {
            return Err(Error::MessageTooLarge {
                size: length,
                max: self.max_frame_size,
            });
        }

        if src.len() < 4 + length {
            src.reserve(4 + length - src.len());
            return Ok(None);
        }

        // Skip the length marker
        src.advance(4);

        let data = src.split_to(length);
        bincode::deserialize(&data)
            .map(Some)
            .map_err(|e| Error::Deserialization(format!("Failed to deserialize message: {}", e)))
    }
}

impl Encoder<Message> for MessageCodec {
    type Error = Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<()> {
        let data = bincode::serialize(&item)?;

        let length = data.len();
        if length > self.max_frame_size {
            return Err(Error::MessageTooLarge {
                size: length,
                max: self.max_frame_size,
            });
        }

        dst.reserve(4 + length);
        dst.put_u32(length as u32);
        dst.put_slice(&data);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Payload;

    #[test]
    fn encode_decode() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let message = Message::new(addr, Payload::PeerDiscovery);

        let mut codec = MessageCodec::new();
        let mut buffer = BytesMut::new();

        codec.encode(message.clone(), &mut buffer).unwrap();

        let decoded = codec.decode(&mut buffer).unwrap();
        assert!(decoded.is_some());
        let decoded = decoded.unwrap();
        assert_eq!(decoded.id, message.id);
    }

    #[test]
    fn partial_frame() {
        let mut codec = MessageCodec::new();
        let mut buffer = BytesMut::new();
        buffer.put_u8(0); // Partial length marker

        let result = codec.decode(&mut buffer).unwrap();
        assert!(result.is_none()); // Not enough data
    }
}
