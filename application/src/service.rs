use std::collections::HashMap;

use thiserror::Error;

use protocol_domain::*;

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("Character not found: {0}")]
    CharacterNotFound(u64),

    #[error("Character name already taken: {0}")]
    CharacterNameTaken(String),

    #[error("Invalid character name: {0}")]
    InvalidCharacterName(String),

    #[error("NPC not found: {0}")]
    NpcNotFound(u64),

    #[error("No exit in that direction")]
    NoExit,

    #[error("Combat not found: {0}")]
    CombatNotFound(u64),

    #[error("Target not in same room")]
    TargetNotInSameRoom,

    #[error("Target is dead")]
    TargetDead,

    #[error("Cannot attack yourself")]
    SelfAttack,

    #[error("Item not found: {0}")]
    ItemNotFound(u32),
}

pub struct GameWorld {
    characters: HashMap<u64, Character>,
    world: World,
    combats: HashMap<u64, Combat>,
    /// One allocator for every runtime-created entity - characters AND
    /// dynamically spawned NPCs. They share a single u64 id space because
    /// the presentation layer addresses both as `entity_id`
    /// (see core/presentation), so an id must identify exactly one thing.
    next_entity_id: u64,
    next_combat_id: u64,
    /// Domain events produced by the last batch of GameWorld operations,
    /// waiting to be drained by the caller (e.g. a command handler, which
    /// translates them via `protocol_presentation::translate_event` and
    /// forwards them to connected clients / an embedded engine). See
    /// docs/11-presentation/01-presentation-command-protocol.md.
    pending_events: Vec<DomainEvent>,
}

impl GameWorld {
    pub fn new() -> Self {
        Self {
            characters: HashMap::new(),
            world: World::new(),
            combats: HashMap::new(),
            // Starts above the fixed ids World::initialize() hardcodes for
            // its seeded NPCs (currently 1-4), so those can never collide
            // with anything allocated here.
            next_entity_id: 1000,
            next_combat_id: 1,
            pending_events: Vec::new(),
        }
    }

    /// Take all `DomainEvent`s produced since the last drain. Call this
    /// after each command that mutates the world (see create_character/
    /// move_character/start_combat) to get what happened in a form that can
    /// be translated to `PresentationCommand`s and forwarded to clients.
    pub fn drain_events(&mut self) -> Vec<DomainEvent> {
        std::mem::take(&mut self.pending_events)
    }

    pub fn create_character(&mut self, name: String, class: &str) -> Result<Character, ApplicationError> {
        let class = CharacterClass::from_str(class)
            .ok_or_else(|| ApplicationError::InvalidCharacterName(class.to_string()))?;

        // Check name uniqueness
        if self.characters.values().any(|c| c.name == name) {
            return Err(ApplicationError::CharacterNameTaken(name));
        }

        let mut character = Character::new(name, class);
        character.id = self.next_entity_id;
        self.next_entity_id += 1;

        // Place in starting room
        character.room_id = 1;

        tracing::info!("Created character: {} ({}) in room 1", character.name, character.id);

        self.pending_events.push(DomainEvent::CharacterCreated {
            character_id: character.id,
            name: character.name.clone(),
        });

        Ok(character)
    }

    pub fn add_character(&mut self, character: Character) {
        self.characters.insert(character.id, character);
    }

    pub fn get_character(&self, id: u64) -> Option<&Character> {
        self.characters.get(&id)
    }

    pub fn get_character_mut(&mut self, id: u64) -> Option<&mut Character> {
        self.characters.get_mut(&id)
    }

    pub fn find_character_by_name(&self, name: &str) -> Option<&Character> {
        self.characters.values().find(|c| c.name == name)
    }

    pub fn look_room(&self, room_id: u32) -> Option<RoomInfo> {
        let room = self.world.get_room(room_id)?;

        let players: Vec<PlayerSummary> = self.characters.values()
            .filter(|c| c.room_id == room_id)
            .map(|c| PlayerSummary {
                id: c.id,
                name: c.name.clone(),
                level: c.level,
            })
            .collect();

        let npcs: Vec<NpcSummary> = room.npc_ids.iter()
            .filter_map(|id| self.world.get_npc(*id))
            .map(|npc| NpcSummary {
                id: npc.id,
                name: npc.name.clone(),
                hp: npc.hp,
                max_hp: npc.max_hp,
            })
            .collect();

        let exits: Vec<String> = room.exits.keys()
            .map(|d| format!("{:?}", d).to_lowercase())
            .collect();

        Some(RoomInfo {
            name: room.name.clone(),
            description: room.description.clone(),
            exits,
            players,
            npcs,
        })
    }

    pub fn move_character(
        &mut self,
        character_id: u64,
        direction: Direction,
    ) -> Result<MoveResult, ApplicationError> {
        let character = self.characters.get(&character_id)
            .ok_or(ApplicationError::CharacterNotFound(character_id))?;

        let current_room_id = character.room_id;
        let current_room = self.world.get_room(current_room_id)
            .ok_or(ApplicationError::NoExit)?;

        let new_room_id = *current_room.exits.get(&direction)
            .ok_or(ApplicationError::NoExit)?;

        // Update character position
        if let Some(character) = self.characters.get_mut(&character_id) {
            character.room_id = new_room_id;
        }

        let new_room = self.world.get_room(new_room_id)
            .ok_or(ApplicationError::NoExit)?;

        self.pending_events.push(DomainEvent::PlayerLeftRoom {
            player_id: character_id,
            room_id: current_room_id,
        });
        self.pending_events.push(DomainEvent::PlayerEnteredRoom {
            player_id: character_id,
            room_id: new_room_id,
        });

        Ok(MoveResult {
            from_room_id: current_room_id,
            to_room_id: new_room_id,
            room_name: new_room.name.clone(),
            room_description: new_room.description.clone(),
        })
    }

    pub fn start_combat(
        &mut self,
        attacker_id: u64,
        target_name: &str,
    ) -> Result<CombatInfo, ApplicationError> {
        if attacker_id == 0 {
            return Err(ApplicationError::SelfAttack);
        }

        let attacker_room_id = self.characters.get(&attacker_id)
            .ok_or(ApplicationError::CharacterNotFound(attacker_id))?
            .room_id;

        let target_npc_id = self.world.find_npc_in_room(attacker_room_id, target_name)
            .ok_or_else(|| ApplicationError::NpcNotFound(0))?
            .id;

        let combat_id = self.next_combat_id;
        self.next_combat_id += 1;

        let mut combat = Combat::new(attacker_id, target_npc_id);
        combat.id = combat_id;

        // Both `characters` and `world` are distinct fields of `self`, so
        // these two mutable borrows can coexist - see Combat::process_attack,
        // which is generic over `&mut dyn Combatant` and no longer needs a
        // fabricated fake Character to fight an Npc.
        let attacker = self.characters.get_mut(&attacker_id).unwrap();
        let target_npc = self.world.get_npc_mut(target_npc_id).unwrap();

        let events = combat.process_attack(attacker, target_npc);

        let damage = events.iter().find_map(|e| match e {
            DomainEvent::AttackExecuted { damage, .. } => Some(*damage),
            _ => None,
        }).unwrap_or(0);
        let message = format!("You hit {} for {} damage!", target_npc.name, damage);
        let target_hp = target_npc.hp;
        let target_max_hp = target_npc.max_hp;
        let target_name = target_npc.name.clone();

        self.combats.insert(combat_id, combat);
        self.pending_events.extend(events);

        Ok(CombatInfo {
            combat_id,
            message,
            damage,
            target_name,
            target_hp,
            target_max_hp,
        })
    }

    pub fn get_inventory(&self, character_id: u64) -> Option<&Inventory> {
        self.characters.get(&character_id).map(|c| &c.inventory)
    }

    /// Create an NPC at runtime and place it in `room_id`.
    ///
    /// Stats are the same shape the seeded NPCs in `World::initialize()`
    /// carry, so a spawned NPC is a first-class `Combatant` - it can be
    /// attacked, take damage, and award XP exactly like the fixed ones.
    pub fn spawn_npc(
        &mut self,
        name: String,
        description: String,
        room_id: u32,
        level: u32,
        hp: u32,
        attack: u32,
        defense: u32,
    ) -> Result<u64, ApplicationError> {
        if self.world.get_room(room_id).is_none() {
            return Err(ApplicationError::NoExit);
        }

        let npc_id = self.next_entity_id;
        self.next_entity_id += 1;

        self.world.add_npc(Npc {
            id: npc_id,
            name: name.clone(),
            description,
            room_id,
            hp,
            max_hp: hp,
            level,
            attack,
            defense,
        });

        tracing::info!("Spawned NPC: {} ({}) in room {}", name, npc_id, room_id);
        self.pending_events.push(DomainEvent::NpcSpawned {
            npc_id,
            name,
            room_id,
        });

        Ok(npc_id)
    }

    /// Move an NPC one room in `direction`, if that exit exists.
    pub fn move_npc(
        &mut self,
        npc_id: u64,
        direction: Direction,
    ) -> Result<u32, ApplicationError> {
        let from_room_id = self
            .world
            .get_npc(npc_id)
            .ok_or(ApplicationError::NpcNotFound(npc_id))?
            .room_id;

        let to_room_id = *self
            .world
            .get_room(from_room_id)
            .ok_or(ApplicationError::NoExit)?
            .exits
            .get(&direction)
            .ok_or(ApplicationError::NoExit)?;

        self.world
            .move_npc(npc_id, to_room_id)
            .ok_or(ApplicationError::NpcNotFound(npc_id))?;

        self.pending_events.push(DomainEvent::NpcMoved {
            npc_id,
            from_room_id,
            to_room_id,
        });

        Ok(to_room_id)
    }

    /// Remove an NPC from the world.
    pub fn despawn_npc(&mut self, npc_id: u64) -> Result<(), ApplicationError> {
        let room_id = self
            .world
            .remove_npc(npc_id)
            .ok_or(ApplicationError::NpcNotFound(npc_id))?;
        self.pending_events
            .push(DomainEvent::NpcDespawned { npc_id, room_id });
        Ok(())
    }

    /// Directions an NPC can currently move, as lowercase strings.
    pub fn npc_exits(&self, npc_id: u64) -> Vec<String> {
        let Some(npc) = self.world.get_npc(npc_id) else {
            return Vec::new();
        };
        self.world
            .exits_from(npc.room_id)
            .into_iter()
            .map(|(d, _)| format!("{:?}", d).to_lowercase())
            .collect()
    }

    pub fn get_world(&self) -> &World {
        &self.world
    }
}

impl Default for GameWorld {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct RoomInfo {
    pub name: String,
    pub description: String,
    pub exits: Vec<String>,
    pub players: Vec<PlayerSummary>,
    pub npcs: Vec<NpcSummary>,
}

#[derive(Debug, Clone)]
pub struct PlayerSummary {
    pub id: u64,
    pub name: String,
    pub level: u32,
}

#[derive(Debug, Clone)]
pub struct NpcSummary {
    pub id: u64,
    pub name: String,
    pub hp: u32,
    pub max_hp: u32,
}

#[derive(Debug, Clone)]
pub struct MoveResult {
    pub from_room_id: u32,
    pub to_room_id: u32,
    pub room_name: String,
    pub room_description: String,
}

#[derive(Debug, Clone)]
pub struct CombatInfo {
    pub combat_id: u64,
    pub message: String,
    pub damage: u32,
    pub target_name: String,
    pub target_hp: u32,
    pub target_max_hp: u32,
}
