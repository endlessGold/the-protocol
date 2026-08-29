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

    pub fn create_character(
        &mut self,
        name: String,
        class: &str,
    ) -> Result<Character, ApplicationError> {
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

        tracing::info!(
            "Created character: {} ({}) in room 1",
            character.name,
            character.id
        );

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

        let players: Vec<PlayerSummary> = self
            .characters
            .values()
            .filter(|c| c.room_id == room_id)
            .map(|c| PlayerSummary {
                id: c.id,
                name: c.name.clone(),
                level: c.level,
            })
            .collect();

        let npcs: Vec<NpcSummary> = room
            .npc_ids
            .iter()
            .filter_map(|id| self.world.get_npc(*id))
            .map(|npc| NpcSummary {
                id: npc.id,
                name: npc.name.clone(),
                hp: npc.hp,
                max_hp: npc.max_hp,
            })
            .collect();

        let exits: Vec<String> = room
            .exits
            .keys()
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
        let character = self
            .characters
            .get(&character_id)
            .ok_or(ApplicationError::CharacterNotFound(character_id))?;

        let current_room_id = character.room_id;
        let current_room = self
            .world
            .get_room(current_room_id)
            .ok_or(ApplicationError::NoExit)?;

        let new_room_id = *current_room
            .exits
            .get(&direction)
            .ok_or(ApplicationError::NoExit)?;

        // Update character position
        if let Some(character) = self.characters.get_mut(&character_id) {
            character.room_id = new_room_id;
        }

        let new_room = self
            .world
            .get_room(new_room_id)
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

        let attacker_room_id = self
            .characters
            .get(&attacker_id)
            .ok_or(ApplicationError::CharacterNotFound(attacker_id))?
            .room_id;

        let target_npc_id = self
            .world
            .find_npc_in_room(attacker_room_id, target_name)
            .ok_or(ApplicationError::NpcNotFound(0))?
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

        let damage = events
            .iter()
            .find_map(|e| match e {
                DomainEvent::AttackExecuted { damage, .. } => Some(*damage),
                _ => None,
            })
            .unwrap_or(0);
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

    /// Create an NPC at runtime and place it in `spec.room_id`.
    ///
    /// Stats are the same shape the seeded NPCs in `World::initialize()`
    /// carry, so a spawned NPC is a first-class `Combatant` - it can be
    /// attacked, take damage, and award XP exactly like the fixed ones.
    pub fn spawn_npc(&mut self, spec: NpcSpawnSpec) -> Result<u64, ApplicationError> {
        if self.world.get_room(spec.room_id).is_none() {
            return Err(ApplicationError::NoExit);
        }

        let npc_id = self.next_entity_id;
        self.next_entity_id += 1;

        self.world.add_npc(Npc {
            id: npc_id,
            name: spec.name.clone(),
            description: spec.description,
            room_id: spec.room_id,
            hp: spec.hp,
            max_hp: spec.hp,
            level: spec.level,
            attack: spec.attack,
            defense: spec.defense,
        });

        tracing::info!(
            "Spawned NPC: {} ({}) in room {}",
            spec.name,
            npc_id,
            spec.room_id
        );
        self.pending_events.push(DomainEvent::NpcSpawned {
            npc_id,
            name: spec.name,
            room_id: spec.room_id,
        });

        Ok(npc_id)
    }

    /// Move an NPC one room in `direction`, if that exit exists.
    pub fn move_npc(&mut self, npc_id: u64, direction: Direction) -> Result<u32, ApplicationError> {
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

/// Everything needed to create an NPC at runtime.
///
/// A struct rather than positional parameters because the alternative was
/// eight arguments, five of them integers - `spawn_npc(name, desc, 1, 3,
/// 40, 7, 3)` gives a reader no way to tell level from hp from attack.
#[derive(Debug, Clone)]
pub struct NpcSpawnSpec {
    pub name: String,
    pub description: String,
    pub room_id: u32,
    pub level: u32,
    /// Starting HP, also used as max_hp.
    pub hp: u32,
    pub attack: u32,
    pub defense: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, room_id: u32) -> NpcSpawnSpec {
        NpcSpawnSpec {
            name: name.to_string(),
            description: "spawned in a test".to_string(),
            room_id,
            level: 2,
            hp: 30,
            attack: 6,
            defense: 2,
        }
    }

    fn new_hero(world: &mut GameWorld) -> u64 {
        let character = world
            .create_character("Hero".to_string(), "warrior")
            .unwrap();
        let id = character.id;
        world.add_character(character);
        world.drain_events(); // discard CharacterCreated
        id
    }

    #[test]
    fn create_character_emits_character_created() {
        let mut world = GameWorld::new();
        let character = world
            .create_character("Hero".to_string(), "warrior")
            .unwrap();

        let events = world.drain_events();
        assert_eq!(events.len(), 1, "got {:?}", events);
        match &events[0] {
            DomainEvent::CharacterCreated { character_id, name } => {
                assert_eq!(*character_id, character.id);
                assert_eq!(name, "Hero");
            }
            other => panic!("expected CharacterCreated, got {:?}", other),
        }
    }

    #[test]
    fn drain_events_empties_the_queue() {
        let mut world = GameWorld::new();
        world
            .create_character("Hero".to_string(), "warrior")
            .unwrap();
        assert!(!world.drain_events().is_empty());
        assert!(
            world.drain_events().is_empty(),
            "a second drain must return nothing"
        );
    }

    #[test]
    fn duplicate_character_names_are_rejected() {
        let mut world = GameWorld::new();
        let first = world
            .create_character("Hero".to_string(), "warrior")
            .unwrap();
        world.add_character(first);
        assert!(matches!(
            world.create_character("Hero".to_string(), "warrior"),
            Err(ApplicationError::CharacterNameTaken(_))
        ));
    }

    #[test]
    fn unknown_class_is_rejected() {
        let mut world = GameWorld::new();
        assert!(world
            .create_character("Hero".to_string(), "wizardish")
            .is_err());
    }

    /// Characters and runtime-spawned NPCs draw from ONE id space, because
    /// the presentation layer addresses both as `entity_id`. A collision
    /// means two different things answer to the same id.
    #[test]
    fn characters_and_npcs_never_share_an_id() {
        let mut world = GameWorld::new();
        let mut seen: Vec<u64> = world.get_world().npcs.keys().copied().collect();

        for i in 0..5 {
            let c = world
                .create_character(format!("Hero{}", i), "warrior")
                .unwrap();
            seen.push(c.id);
            world.add_character(c);

            let npc_id = world.spawn_npc(spec(&format!("Mob{}", i), 1)).unwrap();
            seen.push(npc_id);
        }

        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(before, seen.len(), "duplicate entity id allocated");
    }

    #[test]
    fn runtime_ids_stay_clear_of_the_seeded_npc_range() {
        let mut world = GameWorld::new();
        let seeded_max = world.get_world().npcs.keys().copied().max().unwrap_or(0);

        let npc_id = world.spawn_npc(spec("Wisp", 1)).unwrap();
        assert!(
            npc_id > seeded_max,
            "spawned npc id {} collides with seeded range (max {})",
            npc_id,
            seeded_max
        );
    }

    #[test]
    fn spawn_npc_emits_npc_spawned_and_is_visible_in_the_room() {
        let mut world = GameWorld::new();
        let npc_id = world.spawn_npc(spec("Wisp", 3)).unwrap();

        let events = world.drain_events();
        assert!(
            events.iter().any(|e| matches!(
                e,
                DomainEvent::NpcSpawned { npc_id: id, room_id, .. }
                    if *id == npc_id && *room_id == 3
            )),
            "got {:?}",
            events
        );

        let room = world.look_room(3).expect("room 3 exists");
        assert!(room.npcs.iter().any(|n| n.id == npc_id));
    }

    #[test]
    fn spawn_npc_into_an_unknown_room_fails() {
        let mut world = GameWorld::new();
        assert!(world.spawn_npc(spec("Nowhere", 9999)).is_err());
        assert!(
            world.drain_events().is_empty(),
            "a failed spawn must not emit events"
        );
    }

    #[test]
    fn move_npc_emits_npc_moved_with_both_rooms() {
        let mut world = GameWorld::new();
        let npc_id = world.spawn_npc(spec("Wisp", 1)).unwrap();
        world.drain_events();

        // Room 1 has a north exit to room 2 per World::initialize().
        let to = world.move_npc(npc_id, Direction::North).unwrap();
        assert_eq!(to, 2);

        let events = world.drain_events();
        assert!(
            events.iter().any(|e| matches!(
                e,
                DomainEvent::NpcMoved { from_room_id, to_room_id, .. }
                    if *from_room_id == 1 && *to_room_id == 2
            )),
            "got {:?}",
            events
        );
        assert!(world
            .look_room(2)
            .unwrap()
            .npcs
            .iter()
            .any(|n| n.id == npc_id));
        assert!(!world
            .look_room(1)
            .unwrap()
            .npcs
            .iter()
            .any(|n| n.id == npc_id));
    }

    #[test]
    fn move_npc_through_a_missing_exit_fails_without_events() {
        let mut world = GameWorld::new();
        // Room 3 (Blacksmith) only has a west exit.
        let npc_id = world.spawn_npc(spec("Wisp", 3)).unwrap();
        world.drain_events();

        assert!(world.move_npc(npc_id, Direction::North).is_err());
        assert!(world.drain_events().is_empty());
    }

    #[test]
    fn despawn_npc_emits_and_removes() {
        let mut world = GameWorld::new();
        let npc_id = world.spawn_npc(spec("Wisp", 1)).unwrap();
        world.drain_events();

        world.despawn_npc(npc_id).unwrap();

        let events = world.drain_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, DomainEvent::NpcDespawned { npc_id: id, .. } if *id == npc_id)));
        assert!(!world
            .look_room(1)
            .unwrap()
            .npcs
            .iter()
            .any(|n| n.id == npc_id));
        assert!(world.despawn_npc(npc_id).is_err(), "second despawn errors");
    }

    #[test]
    fn move_character_emits_leave_then_enter() {
        let mut world = GameWorld::new();
        let hero = new_hero(&mut world);

        let result = world.move_character(hero, Direction::North).unwrap();
        assert_eq!(result.from_room_id, 1);
        assert_eq!(result.to_room_id, 2);

        let events = world.drain_events();
        // Order matters: a client applying these in sequence should see the
        // player leave before arriving.
        let left = events
            .iter()
            .position(|e| matches!(e, DomainEvent::PlayerLeftRoom { .. }));
        let entered = events
            .iter()
            .position(|e| matches!(e, DomainEvent::PlayerEnteredRoom { .. }));
        assert!(left.is_some() && entered.is_some(), "got {:?}", events);
        assert!(left < entered, "LeaveRoom should precede EnterRoom");
    }

    #[test]
    fn move_character_through_a_missing_exit_fails() {
        let mut world = GameWorld::new();
        let hero = new_hero(&mut world);
        // Room 1 has no west exit.
        assert!(world.move_character(hero, Direction::West).is_err());
        assert!(world.drain_events().is_empty());
    }

    #[test]
    fn start_combat_hits_a_real_npc_and_emits_attack_executed() {
        let mut world = GameWorld::new();
        let hero = new_hero(&mut world);
        // Town Guard (id 1) is in room 1, where new characters start.
        let info = world.start_combat(hero, "guard").expect("guard is here");

        assert!(info.damage >= 1);
        assert!(info.target_hp < info.target_max_hp);

        let events = world.drain_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DomainEvent::AttackExecuted { .. })),
            "got {:?}",
            events
        );
    }

    #[test]
    fn attacking_something_not_in_the_room_fails() {
        let mut world = GameWorld::new();
        let hero = new_hero(&mut world);
        // The Goblin is in room 5, not room 1.
        assert!(world.start_combat(hero, "goblin").is_err());
        assert!(world.start_combat(hero, "nothing at all").is_err());
    }

    #[test]
    fn get_inventory_returns_the_characters_own() {
        let mut world = GameWorld::new();
        let hero = new_hero(&mut world);
        assert!(world.get_inventory(hero).is_some());
        assert!(world.get_inventory(999_999).is_none());
    }
}
