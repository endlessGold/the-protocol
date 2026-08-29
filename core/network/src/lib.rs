pub mod tcp;
pub mod udp;

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::{BufMut, BytesMut};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use protocol_protocol::{Command, CommandResponse, Message, MessageType, ProtocolCodec};
use protocol_routing::CommandRouter;
use protocol_session::SessionManager;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Codec error: {0}")]
    Codec(#[from] protocol_protocol::codec::CodecError),

    #[error("Session error: {0}")]
    Session(#[from] protocol_session::SessionError),

    #[error("Connection closed")]
    Closed,
}

pub struct NetworkManager {
    tcp_listener: Option<TcpListener>,
    session_manager: Arc<SessionManager>,
    command_router: Arc<CommandRouter>,
    codec: ProtocolCodec,
}

impl NetworkManager {
    pub async fn new(
        bind_address: &str,
        session_manager: Arc<SessionManager>,
        command_router: Arc<CommandRouter>,
    ) -> Result<Self, NetworkError> {
        let tcp_listener = TcpListener::bind(bind_address).await?;
        tracing::info!("TCP listening on {}", bind_address);

        Ok(Self {
            tcp_listener: Some(tcp_listener),
            session_manager,
            command_router,
            codec: ProtocolCodec::new(),
        })
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.tcp_listener.as_ref().and_then(|l| l.local_addr().ok())
    }

    pub async fn accept_connections(&self) -> Result<(), NetworkError> {
        let listener = self.tcp_listener.as_ref().ok_or(NetworkError::Closed)?;

        loop {
            let (socket, addr) = listener.accept().await?;

            if !self.session_manager.can_accept() {
                tracing::warn!("Connection limit reached, rejecting {}", addr);
                drop(socket);
                continue;
            }

            socket.set_nodelay(true)?;

            let session_manager = self.session_manager.clone();
            let command_router = self.command_router.clone();
            let codec = self.codec.clone();

            tokio::spawn(async move {
                if let Err(e) =
                    Self::handle_connection(socket, addr, codec, session_manager, command_router)
                        .await
                {
                    tracing::error!("Connection error from {}: {}", addr, e);
                }
            });
        }
    }

    async fn handle_connection(
        socket: TcpStream,
        addr: SocketAddr,
        codec: ProtocolCodec,
        session_manager: Arc<SessionManager>,
        command_router: Arc<CommandRouter>,
    ) -> Result<(), NetworkError> {
        let session_id =
            session_manager.create_session(addr, protocol_session::TransportType::Tcp)?;

        let (mut reader, mut writer) = socket.into_split();

        // Handshake: read Hello
        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf).await?;
        let total_len = u32::from_be_bytes(len_buf) as usize;

        let mut frame = vec![0u8; total_len - 4];
        reader.read_exact(&mut frame).await?;

        let mut full_frame = BytesMut::with_capacity(4 + total_len);
        full_frame.put_slice(&len_buf);
        full_frame.put_slice(&frame);

        let mut buf = full_frame;
        let _hello_msg = ProtocolCodec::decode_simple(&mut buf)?.ok_or(NetworkError::Closed)?;

        // Send HelloAck
        let hello_ack = Message::hello_ack(session_id, vec!["game".to_string()]);
        let ack_bytes = codec.encode(&hello_ack)?;
        writer.write_all(&ack_bytes).await?;

        // Mark session as authenticated. `session_manager.get()` returns a
        // *clone* of the session, so mutating it directly (the old code here)
        // was a no-op - go through the manager so the change is persisted.
        session_manager.set_state(session_id, protocol_session::SessionState::Authenticated)?;

        tracing::info!("Session {} handshake complete", session_id);

        // Main read loop
        let incoming_rx = {
            let session = session_manager
                .get(session_id)
                .ok_or(NetworkError::Closed)?;
            session.incoming_rx.clone()
        };

        loop {
            let mut len_buf = [0u8; 4];
            tokio::select! {
                result = reader.read_exact(&mut len_buf) => {
                    match result {
                        Ok(_) => {
                            let total_len = u32::from_be_bytes(len_buf) as usize;
                            let mut frame = vec![0u8; total_len - 4];
                            reader.read_exact(&mut frame).await?;

                            let mut full_frame = BytesMut::with_capacity(4 + total_len);
                            full_frame.put_slice(&len_buf);
                            full_frame.put_slice(&frame);

                            let mut buf = full_frame;
                            if let Some(message) = ProtocolCodec::decode_simple(&mut buf)? {
                                match message.message_type {
                                    MessageType::Command => {
                                        let reply = Self::route_command(
                                            &command_router,
                                            session_id,
                                            &message.payload,
                                        ).await;
                                        let encoded = codec.encode(&reply)?;
                                        if writer.write_all(&encoded).await.is_err() {
                                            break;
                                        }
                                    }
                                    MessageType::Ping => {
                                        let encoded = codec.encode(&Message::pong())?;
                                        if writer.write_all(&encoded).await.is_err() {
                                            break;
                                        }
                                    }
                                    MessageType::Disconnect => {
                                        tracing::info!("Session {} requested disconnect", session_id);
                                        break;
                                    }
                                    other => {
                                        tracing::debug!(
                                            "Session {} sent unhandled message type {:?}",
                                            session_id,
                                            other
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::info!("Session {} disconnected: {}", session_id, e);
                            break;
                        }
                    }
                }
                outgoing = async {
                    let mut guard = incoming_rx.lock().await;
                    guard.recv().await
                } => {
                    match outgoing {
                        Some(msg) => {
                            let encoded = codec.encode(&msg)?;
                            if writer.write_all(&encoded).await.is_err() {
                                break;
                            }
                        }
                        None => {
                            tracing::info!("Session {} channel closed", session_id);
                            break;
                        }
                    }
                }
            }
        }

        session_manager.remove(session_id);
        Ok(())
    }

    /// Deserialize a `Command` from a decoded message's payload and dispatch it
    /// through the `CommandRouter`, always producing a reply `Message` (a
    /// `CommandResponse` on success or handler failure, an `Error` message if
    /// the payload itself couldn't be parsed as a `Command`).
    async fn route_command(
        command_router: &CommandRouter,
        session_id: u64,
        payload: &[u8],
    ) -> Message {
        let command: Command = match rmp_serde::from_slice(payload) {
            Ok(command) => command,
            Err(e) => {
                tracing::warn!(
                    "Session {} sent an invalid command payload: {}",
                    session_id,
                    e
                );
                return Message::error(format!("Invalid command payload: {}", e));
            }
        };

        let command_id = command.id;
        let command_type = command.command_type.clone();

        let response = match command_router.route(command, session_id).await {
            Ok(response) => response,
            Err(e) => CommandResponse {
                id: command_id,
                command_type,
                success: false,
                payload: vec![],
                error: Some(e.to_string()),
            },
        };

        Message::command_response(response)
    }
}
