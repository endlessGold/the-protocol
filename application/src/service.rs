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
    next_character_id: u64,
    next_combat_id: u64,
}

impl GameWorld {
    pub fn new() -> Self {
        Self {
            characters: HashMap::new(),
            world: World::new(),
            combats: HashMap::new(),
            next_character_id: 1,
            next_combat_id: 1,
        }
    }

    pub fn create_character(&mut self, name: String, class: &str) -> Result<Character, ApplicationError> {
        let class = CharacterClass::from_str(class)
            .ok_or_else(|| ApplicationError::InvalidCharacterName(class.to_string()))?;

        // Check name uniqueness
        if self.characters.values().any(|c| c.name == name) {
            return Err(ApplicationError::CharacterNameTaken(name));
        }

        let mut character = Character::new(name, class);
        character.id = self.next_character_id;
        self.next_character_id += 1;

        // Place in starting room
        character.room_id = 1;

        tracing::info!("Created character: {} ({}) in room 1", character.name, character.id);
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
        let attacker = self.characters.get(&attacker_id)
            .ok_or(ApplicationError::CharacterNotFound(attacker_id))?
            .clone();

        if attacker_id == 0 {
            return Err(ApplicationError::SelfAttack);
        }

        let target_npc = self.world.find_npc_in_room(attacker.room_id, target_name)
            .ok_or_else(|| ApplicationError::NpcNotFound(0))?
            .clone();

        let combat_id = self.next_combat_id;
        self.next_combat_id += 1;

        let mut combat = Combat::new(attacker_id, target_npc.id);
        combat.id = combat_id;

        // Process attack
        let attacker = self.characters.get_mut(&attacker_id).unwrap();
        let target_npc = self.world.get_npc_mut(target_npc.id).unwrap();

        // Simple attack for now - create a temporary character-like struct for damage calc
        let target_stats = Stats {
            strength: 5,
            dexterity: 5,
            intelligence: 5,
            wisdom: 5,
            constitution: 10,
        };

        let damage = Combat::calculate_damage(attacker, &Character {
            id: target_npc.id,
            name: target_npc.name.clone(),
            class: CharacterClass::Warrior,
            level: 1,
            experience: 0,
            hp: target_npc.hp,
            max_hp: target_npc.max_hp,
            mp: 0,
            max_mp: 0,
            stats: target_stats,
            room_id: target_npc.room_id,
            inventory: Inventory::new(),
        });

        target_npc.hp = target_npc.hp.saturating_sub(damage);

        let message = format!("You hit {} for {} damage!", target_npc.name, damage);
        let target_hp = target_npc.hp;
        let target_name = target_npc.name.clone();

        self.combats.insert(combat_id, combat);

        Ok(CombatInfo {
            combat_id,
            message,
            damage,
            target_name,
            target_hp,
            target_max_hp: target_npc.max_hp,
        })
    }

    pub fn get_inventory(&self, character_id: u64) -> Option<&Inventory> {
        self.characters.get(&character_id).map(|c| &c.inventory)
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
