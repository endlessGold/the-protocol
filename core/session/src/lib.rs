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

    /// Mutate the stored session's state in place. `get()` returns a clone, so
    /// mutating that clone (e.g. `session.set_state(...)`) is a no-op on the
    /// session actually held by the manager - this goes through
    /// `DashMap::get_mut` instead so the change is actually persisted.
    pub fn set_state(&self, session_id: u64, state: SessionState) -> Result<(), SessionError> {
        self.sessions
            .get_mut(&session_id)
            .map(|mut s| s.set_state(state))
            .ok_or(SessionError::NotFound(session_id))
    }

    /// Associate a session with a player/character id, in place (see `set_state`).
    pub fn set_player(&self, session_id: u64, player_id: u64) -> Result<(), SessionError> {
        self.sessions
            .get_mut(&session_id)
            .map(|mut s| s.set_player(player_id))
            .ok_or(SessionError::NotFound(session_id))
    }

    /// Look up the character/player id bound to a session, if any (via
    /// `set_player`). Used by command handlers to resolve which character a
    /// session's commands act on.
    pub fn get_player_id(&self, session_id: u64) -> Option<u64> {
        self.sessions.get(&session_id).and_then(|s| s.player_id)
    }

    pub fn get_by_address(&self, addr: &SocketAddr) -> Option<u64> {
        self.address_sessions.get(addr).map(|id| *id)
    }

    /// Enumerate the ids of every currently active session. `sessions` is
    /// private and otherwise only exposed through `get`/`remove`/
    /// `broadcast`/`send_to`, none of which let a caller discover *which*
    /// sessions exist - this is for callers that need to decide, per
    /// session, whether to send something (e.g. room-scoped event
    /// targeting in `dispatch_events`, see core/runtime/src/main.rs).
    pub fn session_ids(&self) -> Vec<u64> {
        self.sessions.iter().map(|entry| *entry.key()).collect()
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
