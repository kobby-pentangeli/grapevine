//! Codec for framing and encoding/decoding gossip messages.

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::{Error, Message, Result};

/// Maximum message size (10 MB).
///
/// This constant is used as the default for:
/// - `NodeConfig::max_message_size`
/// - `Tcp::new()` (via `with_max_message_size`)
///
/// Users can configure a smaller limit via `NodeConfigBuilder::max_message_size()`,
/// but this represents the absolute maximum enforced by the codec.
pub const MAX_FRAME_SIZE: usize = 10 * 1024 * 1024; // 10 MB

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
        let length = usize::try_from(u32::from_be_bytes(length_bytes)).map_err(|_| {
            Error::MessageTooLarge {
                size: usize::MAX,
                max: self.max_frame_size,
            }
        })?;

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
        bincode::serde::decode_from_slice(&data, bincode::config::standard())
            .map(|(msg, _)| Some(msg))
            .map_err(|e| Error::Deserialization(format!("Failed to deserialize message: {e}")))
    }
}

impl Encoder<Message> for MessageCodec {
    type Error = Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<()> {
        let data = bincode::serde::encode_to_vec(&item, bincode::config::standard())?;

        let length = data.len();
        if length > self.max_frame_size {
            return Err(Error::MessageTooLarge {
                size: length,
                max: self.max_frame_size,
            });
        }
        let length_prefix = u32::try_from(length).map_err(|_| Error::MessageTooLarge {
            size: length,
            max: self.max_frame_size,
        })?;

        dst.reserve(4 + length);
        dst.put_u32(length_prefix);
        dst.put_slice(&data);

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use bytes::Bytes;

    use super::*;
    use crate::Payload;

    #[test]
    fn encode_decode_peer_list_request() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let message = Message::new(addr, Payload::PeerListRequest);

        let mut codec = MessageCodec::new();
        let mut buffer = BytesMut::new();

        codec.encode(message.clone(), &mut buffer).unwrap();

        let decoded = codec.decode(&mut buffer).unwrap();
        assert!(decoded.is_some());
        let decoded = decoded.unwrap();
        assert_eq!(decoded.id, message.id);
    }

    #[test]
    fn encode_decode_application_data() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let data = Bytes::from("Hello, Grapevine!");
        let message = Message::new(addr, Payload::Application(data.clone()));

        let mut codec = MessageCodec::new();
        let mut buffer = BytesMut::new();

        codec.encode(message.clone(), &mut buffer).unwrap();

        let decoded = codec.decode(&mut buffer).unwrap().unwrap();
        match &decoded.payload {
            Payload::Application(d) => assert_eq!(d, &data),
            _ => panic!("Expected Application payload"),
        }
    }

    #[test]
    fn encode_decode_heartbeat() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let message = Message::new(addr, Payload::Heartbeat { from: addr });

        let mut codec = MessageCodec::new();
        let mut buffer = BytesMut::new();

        codec.encode(message.clone(), &mut buffer).unwrap();

        let decoded = codec.decode(&mut buffer).unwrap().unwrap();
        assert_eq!(decoded.id, message.id);
    }

    #[test]
    fn encode_decode_peer_list() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let peers = vec![
            "127.0.0.1:8001".parse().unwrap(),
            "127.0.0.1:8002".parse().unwrap(),
        ];
        let message = Message::new(
            addr,
            Payload::PeerListResponse {
                peers: peers.clone(),
            },
        );

        let mut codec = MessageCodec::new();
        let mut buffer = BytesMut::new();

        codec.encode(message.clone(), &mut buffer).unwrap();

        let decoded = codec.decode(&mut buffer).unwrap().unwrap();
        match &decoded.payload {
            Payload::PeerListResponse { peers: p } => assert_eq!(p, &peers),
            _ => panic!("Expected PeerListResponse payload"),
        }
    }

    #[test]
    fn partial_frame() {
        let mut codec = MessageCodec::new();
        let mut buffer = BytesMut::new();
        buffer.put_u8(0); // Partial length marker

        let result = codec.decode(&mut buffer).unwrap();
        assert!(result.is_none()); // Not enough data
    }

    #[test]
    fn partial_peer_list_request_message() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let message = Message::new(addr, Payload::PeerListRequest);

        let mut codec = MessageCodec::new();
        let mut buffer = BytesMut::new();

        // Encode full message
        codec.encode(message, &mut buffer).unwrap();

        // Only provide partial data
        let partial = buffer.split_to(buffer.len() / 2);
        let mut codec2 = MessageCodec::new();
        let result = codec2.decode(&mut partial.clone()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn message_too_large() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let large_data = Bytes::from(vec![0u8; 11 * 1024 * 1024]); // 11 MB
        let message = Message::new(addr, Payload::Application(large_data));

        let mut codec = MessageCodec::new();
        let mut buffer = BytesMut::new();

        let result = codec.encode(message, &mut buffer);
        assert!(matches!(result, Err(Error::MessageTooLarge { .. })));
    }

    #[test]
    fn custom_max_frame_size() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let data = Bytes::from(vec![0u8; 2000]);
        let message = Message::new(addr, Payload::Application(data));

        let mut codec = MessageCodec::with_max_frame_size(1000);
        let mut buffer = BytesMut::new();

        let result = codec.encode(message, &mut buffer);
        assert!(matches!(result, Err(Error::MessageTooLarge { .. })));
    }

    #[test]
    fn multiple_messages_in_buffer() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let msg1 = Message::new(addr, Payload::PeerListRequest);
        let msg2 = Message::new(addr, Payload::Heartbeat { from: addr });

        let mut codec = MessageCodec::new();
        let mut buffer = BytesMut::new();

        codec.encode(msg1.clone(), &mut buffer).unwrap();
        codec.encode(msg2.clone(), &mut buffer).unwrap();

        let decoded1 = codec.decode(&mut buffer).unwrap().unwrap();
        assert_eq!(decoded1.id, msg1.id);

        let decoded2 = codec.decode(&mut buffer).unwrap().unwrap();
        assert_eq!(decoded2.id, msg2.id);

        // Buffer should be empty
        assert!(codec.decode(&mut buffer).unwrap().is_none());
    }

    #[test]
    fn decode_with_length_prefix_too_large() {
        let mut codec = MessageCodec::with_max_frame_size(1000);
        let mut buffer = BytesMut::new();

        // Put a length that exceeds max_frame_size
        buffer.put_u32(2000);

        let result = codec.decode(&mut buffer);
        assert!(matches!(result, Err(Error::MessageTooLarge { .. })));
    }
}
