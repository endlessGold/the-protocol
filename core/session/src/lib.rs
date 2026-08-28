use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;

use protocol_protocol::Message;

pub mod session;

pub use session::Session;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("Session not found: {0}")]
    NotFound(u64),

    #[error("Session closed")]
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportType {
    Tcp,
    Udp,
    WebSocket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connected,
    Authenticating,
    Authenticated,
    InGame,
    Disconnected,
}

pub struct SessionManager {
    sessions: DashMap<u64, Session>,
    address_sessions: DashMap<SocketAddr, u64>,
    next_id: AtomicU64,
    max_connections: usize,
}

impl SessionManager {
    pub fn new(max_connections: usize) -> Self {
        Self {
            sessions: DashMap::new(),
            address_sessions: DashMap::new(),
            next_id: AtomicU64::new(1),
            max_connections,
        }
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn can_accept(&self) -> bool {
        self.sessions.len() < self.max_connections
    }

    pub fn create_session(
        &self,
        addr: SocketAddr,
        transport: TransportType,
    ) -> Result<u64, SessionError> {
        if !self.can_accept() {
            return Err(SessionError::Closed);
        }

        let session_id = self.next_id();
        let (tx, rx) = mpsc::channel(256);

        let session = Session::new(session_id, addr, transport, tx, rx);

        self.sessions.insert(session_id, session);
        self.address_sessions.insert(addr, session_id);

        tracing::info!("Session {} created from {} ({:?})", session_id, addr, transport);
        Ok(session_id)
    }

    pub fn get(&self, session_id: u64) -> Option<Session> {
        self.sessions.get(&session_id).map(|s| s.clone())
    }

    pub fn get_by_address(&self, addr: &SocketAddr) -> Option<u64> {
        self.address_sessions.get(addr).map(|id| *id)
    }

    pub fn remove(&self, session_id: u64) -> Option<Session> {
        if let Some(session) = self.sessions.remove(&session_id).map(|(_, s)| s) {
            self.address_sessions.remove(&session.address);
            tracing::info!("Session {} removed", session_id);
            Some(session)
        } else {
            None
        }
    }

    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    pub fn total_connected(&self) -> u64 {
        self.next_id.load(Ordering::Relaxed) - 1
    }

    pub fn broadcast(&self, message: &Message, exclude: Option<u64>) {
        for entry in self.sessions.iter() {
            if Some(*entry.key()) != exclude {
                let _ = entry.value().send(message.clone());
            }
        }
    }

    pub fn send_to(&self, session_id: u64, message: Message) -> Result<(), SessionError> {
        if let Some(session) = self.sessions.get(&session_id) {
            session.send(message)?;
            Ok(())
        } else {
            Err(SessionError::NotFound(session_id))
        }
    }
}
