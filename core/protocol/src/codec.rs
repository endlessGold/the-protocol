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

/// Byte-exact tests pinning the MessagePack encoding that non-Rust clients
/// have to reimplement by hand.
///
/// The Godot client (`autoload/core_client.gd` in endlessGold/godot-client)
/// hand-rolls a MessagePack encoder for its Hello handshake and a minimal
/// decoder for Event frames, because GDScript has no MessagePack support.
/// Its byte-level assumptions are invisible to `cargo test` unless pinned
/// here - a change on this side that "just works" for every Rust caller
/// (adding a field, reordering, switching to `to_vec_named`, adding
/// `serde_bytes`) would silently break that client with no failing test
/// anywhere. These tests exist to fail loudly instead. If one breaks, the
/// GDScript client needs the matching update.
#[cfg(test)]
mod wire_format_contract {
    use crate::message::{ClientType, Event, Hello, Message, MessageType};

    #[test]
    fn hello_payload_is_a_positional_array_with_a_named_enum_variant() {
        let message = Message::hello(ClientType::MUD, None);
        assert_eq!(message.message_type, MessageType::Hello);

        // `Message::hello` builds `Hello { protocol_version: 1,
        // client_version: env!("CARGO_PKG_VERSION"), client_type, auth_token }`
        // and encodes it with plain `rmp_serde::to_vec` (NOT `to_vec_named`),
        // so it's a positional 4-element array, not a map of field names.
        let expected_version = env!("CARGO_PKG_VERSION");
        let mut expected = vec![0x94, 0x01]; // fixarray(4), then u8 1
        expected.push(0xa0 | expected_version.len() as u8); // fixstr(len)
        expected.extend_from_slice(expected_version.as_bytes());
        expected.extend_from_slice(&[0xa3, b'M', b'U', b'D']); // fixstr(3) "MUD"
        expected.push(0xc0); // nil (auth_token: None)

        assert_eq!(
            message.payload, expected,
            "Hello payload encoding changed - godot-client's \
             _build_hello_payload() must be updated to match"
        );
    }

    #[test]
    fn event_payload_is_an_array_of_byte_integers_not_a_bin_blob() {
        // `Event.payload` is a plain `Vec<u8>` with no
        // `#[serde(with = "serde_bytes")]`, so serde's generic sequence path
        // encodes it as an ARRAY OF INTEGERS, not a compact bin8/16/32 blob.
        // This is the same "Vec<u8> silently becomes an array" behavior that
        // caused the encode() double-serialization bug fixed earlier; the
        // Godot client decodes the array form as its primary path.
        let event = Event {
            id: 1,
            event_type: "presentation_batch".to_string(),
            timestamp: 2,
            source: "server".to_string(),
            payload: vec![0x5b, 0x5d], // b"[]"
            targets: None,
        };
        let encoded = rmp_serde::to_vec(&event).unwrap();

        // fixarray(6): positional [id, event_type, timestamp, source,
        // payload, targets] - field ORDER is part of the contract.
        assert_eq!(encoded[0], 0x96, "Event should encode as a 6-element array");

        // The 2-byte payload appears as fixarray(2) of two ints, i.e.
        // 0x92 0x5b 0x5d - NOT as bin8 (0xc4 0x02 0x5b 0x5d).
        let payload_as_array = [0x92u8, 0x5b, 0x5d];
        let payload_as_bin = [0xc4u8, 0x02, 0x5b, 0x5d];
        assert!(
            encoded
                .windows(payload_as_array.len())
                .any(|w| w == payload_as_array),
            "Event.payload should encode as an array of byte integers - \
             godot-client's _mp_decode_bytes_flexible() depends on this"
        );
        assert!(
            !encoded
                .windows(payload_as_bin.len())
                .any(|w| w == payload_as_bin),
            "Event.payload is now a bin blob; it used to be an array of \
             integers - update godot-client's decoder"
        );
    }

    #[test]
    fn command_is_a_positional_array_with_payload_as_byte_integers() {
        // godot-client's _build_command_payload() hand-encodes this.
        use crate::message::Command;
        let command = Command {
            id: 7,
            command_type: "look".to_string(),
            session_id: 0,
            timestamp: 1,
            payload: vec![0xAB],
        };
        let encoded = rmp_serde::to_vec(&command).unwrap();

        // fixarray(5): [id, command_type, session_id, timestamp, payload]
        assert_eq!(encoded[0], 0x95, "Command should be a 5-element array");
        // payload: fixarray(1) containing uint8 0xAB -> 0x91 0xcc 0xab.
        // NOT bin8 (0xc4 0x01 0xab).
        assert!(
            encoded.windows(3).any(|w| w == [0x91, 0xcc, 0xab]),
            "Command.payload should be an array of byte integers; got {:02x?}",
            encoded
        );
    }

    #[test]
    fn move_command_direction_encodes_as_its_variant_name() {
        // godot-client's DIRECTIONS map hardcodes these exact strings.
        use crate::message::{Direction, MoveCommand};
        let encoded = rmp_serde::to_vec(&MoveCommand {
            direction: Direction::North,
        })
        .unwrap();

        // fixarray(1) then fixstr(5) "North" - a NAME, not an ordinal.
        assert_eq!(
            encoded,
            vec![0x91, 0xa5, b'N', b'o', b'r', b't', b'h'],
            "Direction should encode as its variant name - godot-client's \
             DIRECTIONS map must match these spellings exactly"
        );
    }

    #[test]
    fn create_character_command_is_two_strings() {
        // godot-client's cmd_create_character() hand-encodes this.
        use crate::message::CreateCharacterCommand;
        let encoded = rmp_serde::to_vec(&CreateCharacterCommand {
            name: "Hero".to_string(),
            class: "warrior".to_string(),
        })
        .unwrap();

        let mut expected = vec![0x92, 0xa4];
        expected.extend_from_slice(b"Hero");
        expected.push(0xa7);
        expected.extend_from_slice(b"warrior");
        assert_eq!(encoded, expected);
    }

    #[test]
    fn frame_header_is_fourteen_bytes_before_the_payload() {
        // Every hand-rolled client depends on
        // payload_len = total_length - 14 - 4 (header + trailing checksum).
        let codec = super::ProtocolCodec::new();
        let message = Message::new(MessageType::Ping, vec![1, 2, 3]);
        let encoded = codec.encode(&message).unwrap();

        let total_len = u32::from_be_bytes(encoded[0..4].try_into().unwrap()) as usize;
        assert_eq!(
            total_len,
            encoded.len(),
            "total_length must count the whole frame including itself"
        );
        assert_eq!(total_len - 14 - 4, message.payload.len());
        assert_eq!(encoded[4], message.version);
        assert_eq!(encoded[13], message.message_type as u8);
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
        let decoded = ProtocolCodec::decode_simple(&mut buf).unwrap().unwrap();

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
