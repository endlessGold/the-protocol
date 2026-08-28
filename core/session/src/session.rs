use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::sync::mpsc;

use protocol_protocol::Message;

use super::{SessionState, TransportType};

#[derive(Clone)]
pub struct Session {
    pub id: u64,
    pub player_id: Option<u64>,
    pub address: SocketAddr,
    pub transport: TransportType,
    pub state: SessionState,
    pub connected_at: Instant,
    pub last_activity: std::time::Instant,
    outgoing_tx: mpsc::Sender<Message>,
    pub incoming_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Message>>>,
}

use std::time::Instant;
use tokio::sync::Mutex;

impl Session {
    pub fn new(
        id: u64,
        address: SocketAddr,
        transport: TransportType,
        outgoing_tx: mpsc::Sender<Message>,
        incoming_rx: mpsc::Receiver<Message>,
    ) -> Self {
        Self {
            id,
            player_id: None,
            address,
            transport,
            state: SessionState::Connected,
            connected_at: Instant::now(),
            last_activity: Instant::now(),
            outgoing_tx,
            incoming_rx: Arc::new(Mutex::new(incoming_rx)),
        }
    }

    pub fn send(&self, message: Message) -> Result<(), super::SessionError> {
        self.outgoing_tx
            .try_send(message)
            .map_err(|_| super::SessionError::Closed)
    }

    pub async fn recv(&self) -> Option<Message> {
        let mut rx = self.incoming_rx.lock().await;
        rx.recv().await
    }

    pub fn set_state(&mut self, state: SessionState) {
        self.state = state;
    }

    pub fn set_player(&mut self, player_id: u64) {
        self.player_id = Some(player_id);
        self.state = SessionState::InGame;
    }

    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }
}
