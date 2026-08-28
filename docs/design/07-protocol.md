# 07 - Protocol

## Overview

The Protocol layer defines how messages are serialized, framed, and routed between Runtime nodes and clients. It is transport-agnostic.

## Protocol Design Principles

1. **Transport-agnostic**: Same protocol over TCP, UDP, WebSocket
2. **Binary-first**: MessagePack for efficiency, with optional JSON for debugging
3. **Extensible**: Version field allows evolution without breaking changes
4. **Bidirectional**: Both client and server can send any message type
5. **Typed**: Strong typing prevents protocol confusion attacks

## Message Format

```
┌──────────────┬──────────────┬──────────────┬──────────────┐
│   Length     │  Version     │  Message ID  │   Type       │
│   (4 bytes)  │  (1 byte)    │  (8 bytes)   │   (1 byte)   │
├──────────────┴──────────────┴──────────────┴──────────────┤
│                     Payload (MessagePack)                  │
│                     (variable length)                      │
├──────────────┬─────────────────────────────────────────────┤
│   Checksum   │                                             │
│   (4 bytes)  │                                             │
└──────────────┴─────────────────────────────────────────────┘
```

### Header Fields

| Field | Size | Description |
|-------|------|-------------|
| Length | 4 bytes | Total message length (including header) |
| Version | 1 byte | Protocol version (currently 1) |
| Message ID | 8 bytes | Unique message identifier for request/response matching |
| Type | 1 byte | Message type (see below) |
| Checksum | 4 bytes | CRC32 of payload |

### Message Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageType {
    // Command messages (client → Runtime)
    Command = 0x01,
    CommandResponse = 0x02,

    // Event messages (Runtime → client, or Runtime → Runtime)
    Event = 0x10,
    EventAck = 0x11,

    // System messages
    Ping = 0x20,
    Pong = 0x21,
    Hello = 0x22,
    HelloAck = 0x23,
    Disconnect = 0x24,
    Error = 0x25,

    // Plugin messages
    PluginMessage = 0x30,
    PluginResponse = 0x31,

    // Admin messages
    AdminCommand = 0x40,
    AdminResponse = 0x41,
}
```

## Handshake Protocol

```
Client                              Runtime
  │                                    │
  │──── Hello (client_version) ──────→│
  │                                    │
  │    [Runtime validates version]     │
  │    [Runtime creates session]       │
  │    [Runtime assigns session_id]    │
  │                                    │
  │←── HelloAck (session_id, caps) ───│
  │                                    │
  │──── Pong ─────────────────────────→│
  │                                    │
  │         Connection Ready           │
```

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_version: u8,
    pub client_version: String,
    pub client_type: ClientType,
    pub auth_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HelloAck {
    pub session_id: u64,
    pub protocol_version: u8,
    pub server_time: u64,
    pub capabilities: Vec<String>,
    pub heartbeat_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientType {
    Game,
    MUD,
    Admin,
    Tool,
    Gateway,
    Internal,
}
```

## Command Protocol

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct Command {
    pub id: u64,
    pub command_type: String,
    pub session_id: u64,
    pub timestamp: u64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandResponse {
    pub id: u64,          // matches Command.id
    pub command_type: String,
    pub success: bool,
    pub payload: Vec<u8>,
    pub error: Option<String>,
}
```

### Example Commands

```rust
// Login command
#[derive(Debug, Serialize, Deserialize)]
pub struct LoginCommand {
    pub username: String,
    pub password: String,
}

// Attack command
#[derive(Debug, Serialize, Deserialize)]
pub struct AttackCommand {
    pub target_id: u64,
    pub weapon_id: Option<u32>,
}

// Move command
#[derive(Debug, Serialize, Deserialize)]
pub struct MoveCommand {
    pub direction: Direction,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Direction {
    North, South, East, West, Up, Down,
}
```

## Event Protocol

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub event_type: String,
    pub timestamp: u64,
    pub source: EventSource,
    pub payload: Vec<u8>,
    pub targets: Option<Vec<u64>>,  // None = broadcast
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EventSource {
    Plugin(String),
    Runtime,
    System,
}
```

### Example Events

```rust
// Player joined room
#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerJoinedEvent {
    pub player_id: u64,
    pub player_name: String,
    pub room_id: u32,
}

// Combat event
#[derive(Debug, Serialize, Deserialize)]
pub struct AttackExecutedEvent {
    pub attacker_id: u64,
    pub target_id: u64,
    pub damage: u32,
    pub weapon: String,
    pub result: CombatResult,
}
```

## Codec

```rust
pub struct ProtocolCodec {
    version: u8,
    compression: Option<Compression>,
    encryption: Option<Encryption>,
}

impl ProtocolCodec {
    pub fn encode(&self, message: &Message) -> Result<BytesMut> {
        let mut buf = BytesMut::new();

        // Encode payload
        let payload = rmp_serde::to_vec(&message.payload)?;

        // Calculate checksum
        let checksum = crc32fast::hash(&payload);

        // Write header
        let total_len = 14 + payload.len() + 4; // header + payload + checksum
        buf.put_u32(total_len as u32);
        buf.put_u8(message.version);
        buf.put_u64(message.id);
        buf.put_u8(message.message_type as u8);

        // Write payload
        buf.put_slice(&payload);

        // Write checksum
        buf.put_u32(checksum);

        Ok(buf)
    }

    pub fn decode(&self, buf: &mut BytesMut) -> Result<Option<Message>> {
        if buf.len() < 14 {
            return Ok(None);
        }

        // Read header
        let total_len = buf.get_u32() as usize;
        if buf.len() < total_len {
            return Ok(None); // Need more data
        }

        let version = buf.get_u8();
        let id = buf.get_u64();
        let message_type = MessageType::from_u8(buf.get_u8())
            .ok_or(ProtocolError::InvalidMessageType)?;

        // Read payload
        let payload_len = total_len - 14 - 4;
        let payload = buf.split_to(payload_len).freeze();

        // Verify checksum
        let expected_checksum = buf.get_u32();
        let actual_checksum = crc32fast::hash(&payload);
        if expected_checksum != actual_checksum {
            return Err(ProtocolError::ChecksumMismatch);
        }

        let payload = rmp_serde::from_slice(&payload)?;

        Ok(Some(Message {
            version,
            id,
            message_type,
            payload,
        }))
    }
}
```

## Frame Protocol (TCP)

TCP is stream-based, so we need framing:

```
Frame:
┌──────────────────┬──────────────────┐
│  Frame Length     │  Frame Data      │
│  (4 bytes)        │  (variable)      │
└──────────────────┴──────────────────┘
```

```rust
pub struct TcpFramed {
    socket: TcpStream,
    codec: ProtocolCodec,
}

impl TcpFramed {
    pub async fn read_frame(&mut self) -> Result<Message> {
        // Read 4-byte length prefix
        let len = self.socket.read_u32().await?;

        // Read exactly `len` bytes
        let mut frame = vec![0u8; len as usize];
        self.socket.read_exact(&mut frame).await?;

        // Decode message
        let mut buf = BytesMut::from(&frame[..]);
        self.codec.decode(&mut buf)?
            .ok_or(ProtocolError::IncompleteFrame)
    }

    pub async fn write_frame(&mut self, message: &Message) -> Result<()> {
        let frame = self.codec.encode(message)?;
        self.socket.write_all(&frame).await?;
        self.socket.flush().await?;
        Ok(())
    }
}
```

## UDP Protocol

UDP is datagram-based, so each packet is self-contained:

```
UDP Packet:
┌──────────────┬──────────────┬──────────────┐
│  Header      │  Payload     │  Checksum    │
│  (14 bytes)  │  (variable)  │  (4 bytes)   │
└──────────────┴──────────────┴──────────────┘
```

UDP adds:
- Sequence numbers (for ordering where needed)
- Acknowledgment messages (for reliability when needed)
- Fragmentation support (for large messages)

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct UdpHeader {
    pub sequence: u32,
    pub ack: Option<u32>,
    pub ack_bitfield: u32,
    pub message: Message,
}
```

## WebSocket Protocol

WebSocket provides framing and bidirectional communication natively. Each WebSocket message contains one Protocol message.

```rust
pub struct WebSocketTransport {
    socket: WebSocket<TcpStream>,
    codec: ProtocolCodec,
}

impl WebSocketTransport {
    pub async fn send(&mut self, message: &Message) -> Result<()> {
        let payload = rmp_serde::to_vec(message)?;
        self.socket.send(Message::Binary(payload)).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Option<Message>> {
        match self.socket.recv().await? {
            Some(Message::Binary(data)) => {
                let message = rmp_serde::from_slice(&data)?;
                Ok(Some(message))
            }
            _ => Ok(None),
        }
    }
}
```

## Protocol Versioning

```rust
pub const PROTOCOL_VERSION: u8 = 1;

// Version compatibility:
// Same version: full compatibility
// Different version: graceful degradation or rejection
```

## Security in Protocol

| Feature | Implementation |
|---------|---------------|
| Message integrity | CRC32 checksum in header |
| Replay protection | Timestamp + sequence numbers |
| Rate limiting | Per-session command rate |
| Message size limits | Max 1MB per message |
| Timeout detection | Heartbeat (ping/pong every 30s) |

## References

- [08-network.md](08-network.md) - Network transport layer
- [05-plugin-api.md](05-plugin-api.md) - Plugin message format
- [01-architecture.md](01-architecture.md) - Overall architecture
