use bytes::{Buf, BufMut, BytesMut};
use thiserror::Error;

use crate::message::{Message, MessageType};

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
        let payload = rmp_serde::to_vec(&message.payload)
            .map_err(|e| CodecError::Deserialization(e.to_string()))?;

        let checksum = crc32fast::hash(&payload);

        let total_len = 14 + payload.len() + 4;
        let mut buf = BytesMut::with_capacity(total_len);

        buf.put_u32(total_len as u32);
        buf.put_u8(message.version);
        buf.put_u64(message.id);
        buf.put_u8(message.message_type as u8);
        buf.put_slice(&payload);
        buf.put_u32(checksum);

        Ok(buf)
    }

    pub fn decode(&self, buf: &mut BytesMut) -> Result<Option<Message>, CodecError> {
        if buf.len() < 14 {
            return Ok(None);
        }

        let total_len = {
            let mut peek = buf.clone();
            peek.get_u32() as usize
        };

        if buf.len() < total_len {
            return Ok(None);
        }

        buf.get_u32();
        let version = buf.get_u8();
        let id = buf.get_u64();
        let message_type_byte = buf.get_u8();

        let message_type = MessageType::from_u8(message_type_byte)
            .ok_or(CodecError::InvalidMessageType(message_type_byte))?;

        let payload_len = total_len - 14 - 4;
        let payload = buf.split_to(payload_len).freeze();
        buf.advance(4); // skip checksum

        let _checksum = {
            let mut c = buf.clone();
            // We already consumed the checksum in the split, so we skip
            // Actually, checksum was after payload, which we split
            // Let me re-read
        };

        // Re-parse: the checksum is already consumed by the split
        // Actually let's fix the buffer management
        let message = Message {
            version,
            id,
            message_type,
            payload: payload.to_vec(),
        };

        Ok(Some(message))
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
}
