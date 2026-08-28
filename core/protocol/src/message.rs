use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageType {
    Command = 0x01,
    CommandResponse = 0x02,
    Event = 0x10,
    EventAck = 0x11,
    Ping = 0x20,
    Pong = 0x21,
    Hello = 0x22,
    HelloAck = 0x23,
    Disconnect = 0x24,
    Error = 0x25,
    PluginMessage = 0x30,
    PluginResponse = 0x31,
}

impl MessageType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::Command),
            0x02 => Some(Self::CommandResponse),
            0x10 => Some(Self::Event),
            0x11 => Some(Self::EventAck),
            0x20 => Some(Self::Ping),
            0x21 => Some(Self::Pong),
            0x22 => Some(Self::Hello),
            0x23 => Some(Self::HelloAck),
            0x24 => Some(Self::Disconnect),
            0x25 => Some(Self::Error),
            0x30 => Some(Self::PluginMessage),
            0x31 => Some(Self::PluginResponse),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub version: u8,
    pub id: u64,
    pub message_type: MessageType,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn new(message_type: MessageType, payload: Vec<u8>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            id: rand::random(),
            message_type,
            payload,
        }
    }

    pub fn hello(client_type: ClientType, auth_token: Option<String>) -> Self {
        let hello = Hello {
            protocol_version: PROTOCOL_VERSION,
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            client_type,
            auth_token,
        };
        let payload = rmp_serde::to_vec(&hello).unwrap();
        Self::new(MessageType::Hello, payload)
    }

    pub fn hello_ack(session_id: u64, capabilities: Vec<String>) -> Self {
        let ack = HelloAck {
            session_id,
            protocol_version: PROTOCOL_VERSION,
            server_time: chrono::Utc::now().timestamp_millis() as u64,
            capabilities,
            heartbeat_interval_ms: 30000,
        };
        let payload = rmp_serde::to_vec(&ack).unwrap();
        Self::new(MessageType::HelloAck, payload)
    }

    pub fn command(command: Command) -> Self {
        let payload = rmp_serde::to_vec(&command).unwrap();
        Self::new(MessageType::Command, payload)
    }

    pub fn command_response(response: CommandResponse) -> Self {
        let payload = rmp_serde::to_vec(&response).unwrap();
        Self::new(MessageType::CommandResponse, payload)
    }

    pub fn event(event: Event) -> Self {
        let payload = rmp_serde::to_vec(&event).unwrap();
        Self::new(MessageType::Event, payload)
    }

    pub fn ping() -> Self {
        Self::new(MessageType::Ping, vec![])
    }

    pub fn pong() -> Self {
        Self::new(MessageType::Pong, vec![])
    }

    pub fn disconnect() -> Self {
        Self::new(MessageType::Disconnect, vec![])
    }

    pub fn error(message: String) -> Self {
        let error = ErrorResponse { message };
        let payload = rmp_serde::to_vec(&error).unwrap();
        Self::new(MessageType::Error, payload)
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_version: u8,
    pub client_version: String,
    pub client_type: ClientType,
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloAck {
    pub session_id: u64,
    pub protocol_version: u8,
    pub server_time: u64,
    pub capabilities: Vec<String>,
    pub heartbeat_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub id: u64,
    pub command_type: String,
    pub session_id: u64,
    pub timestamp: u64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResponse {
    pub id: u64,
    pub command_type: String,
    pub success: bool,
    pub payload: Vec<u8>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub event_type: String,
    pub timestamp: u64,
    pub source: String,
    pub payload: Vec<u8>,
    pub targets: Option<Vec<u64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginCommand {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub success: bool,
    pub session_id: u64,
    pub player_id: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCharacterCommand {
    pub name: String,
    pub class: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCharacterResponse {
    pub success: bool,
    pub character_id: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveCommand {
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Direction {
    North,
    South,
    East,
    West,
    Up,
    Down,
}

impl Direction {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "north" | "n" => Some(Self::North),
            "south" | "s" => Some(Self::South),
            "east" | "e" => Some(Self::East),
            "west" | "w" => Some(Self::West),
            "up" | "u" => Some(Self::Up),
            "down" | "d" => Some(Self::Down),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveResponse {
    pub success: bool,
    pub room_name: Option<String>,
    pub room_description: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackCommand {
    pub target_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackResponse {
    pub success: bool,
    pub damage: Option<u32>,
    pub target_hp: Option<u32>,
    pub message: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookResponse {
    pub room_name: String,
    pub room_description: String,
    pub exits: Vec<String>,
    pub players: Vec<PlayerSummary>,
    pub npcs: Vec<NpcSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSummary {
    pub id: u64,
    pub name: String,
    pub level: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcSummary {
    pub id: u64,
    pub name: String,
    pub hp: u32,
    pub max_hp: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryResponse {
    pub items: Vec<InventoryItem>,
    pub gold: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryItem {
    pub item_id: u32,
    pub name: String,
    pub quantity: u32,
    pub item_type: String,
}
