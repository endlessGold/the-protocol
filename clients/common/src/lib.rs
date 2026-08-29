//! Shared client for The Protocol's TCP wire protocol.
//!
//! Exists for two reasons.
//!
//! **Deduplication.** `clients/mud` and `core/runtime`'s `run_client()` were
//! the same program written twice - identical handshake, identical command
//! construction, identical response decoding - with no shared code at all.
//!
//! **A real bug.** Both hand-rolled loops were strictly request/response:
//! write a command, then `read_exact` exactly one frame and treat it as the
//! reply. That was fine when the server only ever spoke when spoken to, but
//! it now pushes asynchronous `Event` frames (presentation batches) down the
//! same socket. A pushed Event arriving between a command and its response
//! got consumed *as* the response, so the client silently mis-parsed it and
//! then read the real response as the reply to the *next* command - drifting
//! one frame further out of step with every event. [`Connection::request`]
//! fixes that by dispatching on message type and skipping past anything
//! that isn't the reply it's waiting for.

use std::collections::VecDeque;

use protocol_presentation::PresentationCommand;
use protocol_protocol::{
    ClientType, Command, CommandResponse, CreateCharacterCommand, Direction, ErrorResponse,
    HelloAck, Message, MessageType, MoveCommand, ProtocolCodec,
};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Codec error: {0}")]
    Codec(#[from] protocol_protocol::codec::CodecError),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Handshake failed: expected HelloAck, got {0:?}")]
    Handshake(MessageType),

    #[error("Server reported: {0}")]
    Server(String),
}

/// Something the server sent that wasn't the reply we were waiting for.
#[derive(Debug, Clone)]
pub enum Pushed {
    /// A batch of presentation commands (`event_type == "presentation_batch"`).
    Presentation(Vec<PresentationCommand>),
    /// Any other event, left undecoded.
    OtherEvent { event_type: String },
    /// The server asked us to go away.
    Disconnect,
}

/// A connected, handshaken session.
pub struct Connection {
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
    codec: ProtocolCodec,
    session_id: u64,
    /// Server-pushed messages seen while waiting for a command response.
    /// Drained by [`Connection::take_pushed`].
    pushed: VecDeque<Pushed>,
}

impl Connection {
    /// Connect and complete the Hello/HelloAck handshake.
    pub async fn connect(addr: &str) -> Result<Self, ClientError> {
        let stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;
        let (reader, writer) = stream.into_split();

        let mut conn = Self {
            reader,
            writer,
            codec: ProtocolCodec::new(),
            session_id: 0,
            pushed: VecDeque::new(),
        };

        conn.send(&Message::hello(ClientType::MUD, None)).await?;

        let ack = conn.read_frame().await?;
        if ack.message_type != MessageType::HelloAck {
            return Err(ClientError::Handshake(ack.message_type));
        }
        let hello_ack: HelloAck = rmp_serde::from_slice(&ack.payload)
            .map_err(|e| ClientError::Serialization(e.to_string()))?;
        conn.session_id = hello_ack.session_id;

        Ok(conn)
    }

    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    /// Server-pushed messages collected since the last call, oldest first.
    pub fn take_pushed(&mut self) -> Vec<Pushed> {
        self.pushed.drain(..).collect()
    }

    /// Send a command and wait for its `CommandResponse`.
    ///
    /// Anything else that arrives first (presentation events, pings) is
    /// queued for [`take_pushed`] rather than being mistaken for the reply -
    /// see the module docs.
    ///
    /// [`take_pushed`]: Connection::take_pushed
    pub async fn request(
        &mut self,
        command_type: &str,
        payload: Vec<u8>,
    ) -> Result<CommandResponse, ClientError> {
        let command = Command {
            id: rand::random(),
            command_type: command_type.to_string(),
            // The server identifies the session from the connection, not
            // from this field.
            session_id: 0,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            payload,
        };
        self.send(&Message::command(command)).await?;

        loop {
            let message = self.read_frame().await?;
            match message.message_type {
                MessageType::CommandResponse => {
                    return rmp_serde::from_slice(&message.payload)
                        .map_err(|e| ClientError::Serialization(e.to_string()));
                }
                MessageType::Error => {
                    let err: ErrorResponse = rmp_serde::from_slice(&message.payload)
                        .map_err(|e| ClientError::Serialization(e.to_string()))?;
                    return Err(ClientError::Server(err.message));
                }
                MessageType::Event => {
                    if let Some(pushed) = decode_event(&message.payload) {
                        self.pushed.push_back(pushed);
                    }
                }
                MessageType::Ping => {
                    self.send(&Message::pong()).await?;
                }
                MessageType::Disconnect => {
                    self.pushed.push_back(Pushed::Disconnect);
                    return Err(ClientError::Server("server disconnected".to_string()));
                }
                other => {
                    tracing::debug!("ignoring unexpected message type {:?}", other);
                }
            }
        }
    }

    async fn send(&mut self, message: &Message) -> Result<(), ClientError> {
        let bytes = self.codec.encode(message)?;
        self.writer.write_all(&bytes).await?;
        Ok(())
    }

    /// Read exactly one length-prefixed frame.
    async fn read_frame(&mut self) -> Result<Message, ClientError> {
        let mut len_buf = [0u8; 4];
        self.reader.read_exact(&mut len_buf).await?;
        let total_len = u32::from_be_bytes(len_buf) as usize;

        let mut rest = vec![0u8; total_len - 4];
        self.reader.read_exact(&mut rest).await?;

        let mut frame = bytes::BytesMut::with_capacity(total_len);
        frame.extend_from_slice(&len_buf);
        frame.extend_from_slice(&rest);

        ProtocolCodec::decode_simple(&mut frame)?
            .ok_or_else(|| ClientError::Serialization("incomplete frame".to_string()))
    }
}

/// Decode an `Event` payload far enough to classify it.
fn decode_event(payload: &[u8]) -> Option<Pushed> {
    let event: protocol_protocol::Event = rmp_serde::from_slice(payload).ok()?;
    if event.event_type == "presentation_batch" {
        // This payload is JSON, not MessagePack - see docs/11-presentation
        // §7.1. It was switched so a GDScript client (no msgpack codec) can
        // read it with Godot's built-in JSON.
        match serde_json::from_slice::<Vec<PresentationCommand>>(&event.payload) {
            Ok(commands) => Some(Pushed::Presentation(commands)),
            Err(e) => {
                tracing::warn!("bad presentation_batch payload: {}", e);
                None
            }
        }
    } else {
        Some(Pushed::OtherEvent {
            event_type: event.event_type,
        })
    }
}

/// Argument encoders for the built-in commands.
///
/// `attack` is deliberately the odd one out: the server's `AttackHandler`
/// reads its payload as a raw UTF-8 string (`from_utf8_lossy`), not
/// MessagePack.
pub mod args {
    use super::*;

    pub fn none() -> Vec<u8> {
        Vec::new()
    }

    pub fn create_character(name: &str, class: &str) -> Result<Vec<u8>, ClientError> {
        rmp_serde::to_vec(&CreateCharacterCommand {
            name: name.to_string(),
            class: class.to_string(),
        })
        .map_err(|e| ClientError::Serialization(e.to_string()))
    }

    pub fn movement(direction: Direction) -> Result<Vec<u8>, ClientError> {
        rmp_serde::to_vec(&MoveCommand { direction })
            .map_err(|e| ClientError::Serialization(e.to_string()))
    }

    pub fn attack(target: &str) -> Vec<u8> {
        target.as_bytes().to_vec()
    }
}

/// Render a presentation command as a line of MUD-style text.
pub fn describe(command: &PresentationCommand) -> String {
    match command {
        PresentationCommand::SpawnEntity { display_name, .. } => {
            format!("{} appears.", display_name)
        }
        PresentationCommand::DespawnEntity { entity_id } => {
            format!("#{} is gone.", entity_id)
        }
        PresentationCommand::EnterRoom { entity_id, room_id } => {
            format!("#{} enters room {}.", entity_id, room_id)
        }
        PresentationCommand::LeaveRoom { entity_id, room_id } => {
            format!("#{} leaves room {}.", entity_id, room_id)
        }
        PresentationCommand::UpdateProperty {
            entity_id,
            key,
            value,
        } => format!("#{} {} is now {:?}.", entity_id, key, value),
        PresentationCommand::PlayEffect { name, .. } => format!("[{}]", name),
        PresentationCommand::ShowMessage { text, .. } => text.clone(),
    }
}
