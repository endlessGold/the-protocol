use bytes::{Buf, BufMut, BytesMut};
use thiserror::Error;

use crate::message::{Message, MessageType};
#[cfg(test)]
use crate::message::{Command, CommandResponse};

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid message type: {0}")]
    InvalidMessageType(u8),

    #[error("Checksum mismatch")]
    ChecksumMismatch,

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Incomplete frame")]
    Incomplete,
}

#[derive(Debug, Clone)]
pub struct ProtocolCodec;

impl ProtocolCodec {
    pub fn new() -> Self {
        Self
    }

    pub fn encode(&self, message: &Message) -> Result<BytesMut, CodecError> {
        // `message.payload` is already a serialized byte buffer (e.g. produced by
        // `Message::command()`/`rmp_serde::to_vec`) - it must be written to the wire
        // as-is. Re-serializing it here (as this used to do via
        // `rmp_serde::to_vec(&message.payload)`) would wrap it as a msgpack array of
        // individual bytes, which `decode()`/`decode_simple()` never unwrap, silently
        // corrupting every payload with non-empty content.
        let checksum = crc32fast::hash(&message.payload);

        let total_len = 14 + message.payload.len() + 4;
        let mut buf = BytesMut::with_capacity(total_len);

        buf.put_u32(total_len as u32);
        buf.put_u8(message.version);
        buf.put_u64(message.id);
        buf.put_u8(message.message_type as u8);
        buf.put_slice(&message.payload);
        buf.put_u32(checksum);

        Ok(buf)
    }

    /// Like `decode_simple`, but additionally verifies the trailing CRC32
    /// checksum against the decoded payload and rejects the frame on mismatch.
    pub fn decode(&self, buf: &mut BytesMut) -> Result<Option<Message>, CodecError> {
        if buf.len() < 4 {
            return Ok(None);
        }

        let total_len = {
            let mut peek = buf.clone();
            peek.get_u32() as usize
        };

        if total_len < 14 + 4 {
            return Err(CodecError::Incomplete);
        }

        if buf.len() < total_len {
            return Ok(None);
        }

        buf.get_u32(); // length (already peeked above)
        let version = buf.get_u8();
        let id = buf.get_u64();
        let message_type_byte = buf.get_u8();

        let message_type = MessageType::from_u8(message_type_byte)
            .ok_or(CodecError::InvalidMessageType(message_type_byte))?;

        let payload_len = total_len - 14 - 4;
        let mut payload = vec![0u8; payload_len];
        buf.copy_to_slice(&mut payload);

        let checksum = buf.get_u32();
        if checksum != crc32fast::hash(&payload) {
            return Err(CodecError::ChecksumMismatch);
        }

        Ok(Some(Message {
            version,
            id,
            message_type,
            payload,
        }))
    }

    pub fn decode_simple(buf: &mut BytesMut) -> Result<Option<Message>, CodecError> {
        if buf.len() < 4 {
            return Ok(None);
        }

        let total_len = {
            let mut peek = buf.clone();
            peek.get_u32() as usize
        };

        if buf.len() < total_len {
            return Ok(None);
        }

        buf.get_u32(); // length
        let version = buf.get_u8();
        let id = buf.get_u64();
        let message_type_byte = buf.get_u8();

        let message_type = MessageType::from_u8(message_type_byte)
            .ok_or(CodecError::InvalidMessageType(message_type_byte))?;

        let payload_len = total_len - 14 - 4;
        let mut payload = vec![0u8; payload_len];
        buf.copy_to_slice(&mut payload);

        let _checksum = buf.get_u32();

        Ok(Some(Message {
            version,
            id,
            message_type,
            payload,
        }))
    }
}

impl Default for ProtocolCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let codec = ProtocolCodec::new();
        let original = Message::ping();

        let encoded = codec.encode(&original).unwrap();
        let mut buf = encoded;
        let decoded = codec.decode_simple(&mut buf).unwrap().unwrap();

        assert_eq!(decoded.version, original.version);
        assert_eq!(decoded.id, original.id);
        assert_eq!(decoded.message_type, original.message_type);
    }

    #[test]
    fn test_encode_decode_roundtrip_preserves_nonempty_payload() {
        // Regression test: encode() used to re-serialize `message.payload` with
        // rmp_serde (turning it into a msgpack array-of-bytes), which decode()
        // never unwrapped - so any message with a real payload (i.e. every
        // Command/CommandResponse/Event) came back corrupted on the other end.
        let codec = ProtocolCodec::new();
        let command = Command {
            id: 42,
            command_type: "move".to_string(),
            session_id: 7,
            timestamp: 1234,
            payload: vec![1, 2, 3, 4, 5],
        };
        let original = Message::command(command.clone());

        let mut encoded = codec.encode(&original).unwrap();
        let decoded = codec.decode(&mut encoded).unwrap().unwrap();

        assert_eq!(decoded.payload, original.payload);
        let round_tripped: Command = rmp_serde::from_slice(&decoded.payload).unwrap();
        assert_eq!(round_tripped.id, command.id);
        assert_eq!(round_tripped.command_type, command.command_type);
        assert_eq!(round_tripped.payload, command.payload);
    }

    #[test]
    fn test_decode_rejects_corrupted_checksum() {
        let codec = ProtocolCodec::new();
        let original = Message::command_response(CommandResponse {
            id: 1,
            command_type: "look".to_string(),
            success: true,
            payload: vec![9, 9, 9],
            error: None,
        });

        let mut encoded = codec.encode(&original).unwrap();
        // Flip a bit in the payload without touching the trailing checksum.
        let payload_start = 14;
        encoded[payload_start] ^= 0xFF;

        let result = codec.decode(&mut encoded);
        assert!(matches!(result, Err(CodecError::ChecksumMismatch)));
    }

    #[test]
    fn test_decode_simple_still_works_without_verification() {
        let codec = ProtocolCodec::new();
        let original = Message::pong();

        let mut encoded = codec.encode(&original).unwrap();
        let decoded = ProtocolCodec::decode_simple(&mut encoded).unwrap().unwrap();

        assert_eq!(decoded.message_type, original.message_type);
        assert_eq!(decoded.payload, original.payload);
    }
}
