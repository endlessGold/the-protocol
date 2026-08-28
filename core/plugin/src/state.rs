use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerData {
    pub id: i64,
    pub name: String,
    pub level: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub room_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryEntry {
    pub item_id: i64,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatState {
    pub combat_id: i64,
    pub attacker_id: i64,
    pub defender_id: i64,
    pub turn: i32,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct HostContext {
    pub player_id: i64,
    pub room_id: i64,
    pub plugin_name: String,
}

#[derive(Debug, Clone)]
pub struct HostState {
    pub context: HostContext,
    pub players: Arc<DashMap<i64, PlayerData>>,
    pub inventories: Arc<DashMap<i64, Vec<InventoryEntry>>>,
    pub combats: Arc<DashMap<i64, CombatState>>,
    pub storage: Arc<DashMap<String, Vec<u8>>>,
    pub events: Arc<DashMap<i64, Vec<String>>>,
    pub messages: Arc<DashMap<i64, Vec<String>>>,
    pub next_combat_id: Arc<parking_lot::Mutex<i64>>,
    pub next_event_id: Arc<parking_lot::Mutex<i64>>,
}

impl HostState {
    pub fn new(context: HostContext, state: Arc<SharedState>) -> Self {
        Self {
            context,
            players: state.players.clone(),
            inventories: state.inventories.clone(),
            combats: state.combats.clone(),
            storage: state.storage.clone(),
            events: state.events.clone(),
            messages: state.messages.clone(),
            next_combat_id: state.next_combat_id.clone(),
            next_event_id: state.next_event_id.clone(),
        }
    }
}

#[derive(Debug, Default)]
pub struct SharedState {
    pub players: Arc<DashMap<i64, PlayerData>>,
    pub inventories: Arc<DashMap<i64, Vec<InventoryEntry>>>,
    pub combats: Arc<DashMap<i64, CombatState>>,
    pub storage: Arc<DashMap<String, Vec<u8>>>,
    pub events: Arc<DashMap<i64, Vec<String>>>,
    pub messages: Arc<DashMap<i64, Vec<String>>>,
    pub next_combat_id: Arc<parking_lot::Mutex<i64>>,
    pub next_event_id: Arc<parking_lot::Mutex<i64>>,
}

impl SharedState {
    pub fn new() -> Self {
        Self::default()
    }
}
