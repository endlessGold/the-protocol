use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEvent {
    CharacterCreated {
        character_id: u64,
        name: String,
    },
    LevelUp {
        character_id: u64,
        new_level: u32,
    },
    CombatStarted {
        combat_id: u64,
        attacker_id: u64,
        target_id: u64,
    },
    AttackExecuted {
        combat_id: u64,
        attacker_id: u64,
        target_id: u64,
        damage: u32,
    },
    CombatEnded {
        combat_id: u64,
        winner_id: u64,
        loser_id: u64,
    },
    PlayerEnteredRoom {
        player_id: u64,
        room_id: u32,
    },
    PlayerLeftRoom {
        player_id: u64,
        room_id: u32,
    },
    ItemAcquired {
        player_id: u64,
        item_id: u32,
        quantity: u32,
    },
    ItemRemoved {
        player_id: u64,
        item_id: u32,
        quantity: u32,
    },
}
