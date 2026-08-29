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

    /// An NPC came into existence at runtime (as opposed to the fixed set
    /// `World::initialize()` seeds). Carries `room_id` because, unlike
    /// `CharacterCreated`, NPCs are not all born in room 1.
    NpcSpawned {
        npc_id: u64,
        name: String,
        room_id: u32,
    },
    NpcMoved {
        npc_id: u64,
        from_room_id: u32,
        to_room_id: u32,
    },
    NpcDespawned {
        npc_id: u64,
        room_id: u32,
    },
}
