use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    North,
    South,
    East,
    West,
    Up,
    Down,
}

impl Direction {
    // Not `std::str::FromStr`: that returns Result, and an unrecognized
    // name here is ordinary user input rather than an error worth a type.
    #[allow(clippy::should_implement_trait)]
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

    pub fn opposite(&self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::East => Self::West,
            Self::West => Self::East,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub exits: HashMap<Direction, u32>,
    pub npc_ids: Vec<u64>,
    pub item_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Npc {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub room_id: u32,
    pub hp: u32,
    pub max_hp: u32,
    /// Used for XP awards when this NPC is defeated (`Combatant::level`).
    pub level: u32,
    /// Effective offensive power for damage calculation (`Combatant::offense`).
    pub attack: u32,
    /// Effective defensive power for damage calculation (`Combatant::defense`).
    pub defense: u32,
}

impl Npc {
    pub fn take_damage(&mut self, amount: u32) -> u32 {
        let actual = amount.min(self.hp);
        self.hp -= actual;
        actual
    }

    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }
}

impl crate::combatant::Combatant for Npc {
    fn combatant_id(&self) -> u64 {
        self.id
    }

    fn combatant_name(&self) -> &str {
        &self.name
    }

    fn hp(&self) -> u32 {
        self.hp
    }

    fn max_hp(&self) -> u32 {
        self.max_hp
    }

    fn level(&self) -> u32 {
        self.level
    }

    fn take_damage(&mut self, amount: u32) -> u32 {
        Npc::take_damage(self, amount)
    }

    fn is_alive(&self) -> bool {
        Npc::is_alive(self)
    }

    fn offense(&self) -> u32 {
        self.attack
    }

    fn defense(&self) -> u32 {
        self.defense
    }

    // grant_experience: default no-op - NPCs don't level up.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub rooms: HashMap<u32, Room>,
    pub npcs: HashMap<u64, Npc>,
}

impl World {
    pub fn new() -> Self {
        let mut world = Self {
            rooms: HashMap::new(),
            npcs: HashMap::new(),
        };
        world.initialize();
        world
    }

    fn initialize(&mut self) {
        // Town Square
        let mut town_exits = HashMap::new();
        town_exits.insert(Direction::North, 2);
        town_exits.insert(Direction::East, 3);
        town_exits.insert(Direction::South, 4);

        self.rooms.insert(1, Room {
            id: 1,
            name: "Town Square".to_string(),
            description: "A bustling town square with a fountain in the center. The water sparkles in the sunlight.".to_string(),
            exits: town_exits,
            npc_ids: vec![1],
            item_ids: vec![],
        });

        // Forest Path
        let mut forest_exits = HashMap::new();
        forest_exits.insert(Direction::South, 1);
        forest_exits.insert(Direction::North, 5);

        self.rooms.insert(
            2,
            Room {
                id: 2,
                name: "Forest Path".to_string(),
                description:
                    "A winding path through a dense forest. Birds chirp in the canopy above."
                        .to_string(),
                exits: forest_exits,
                npc_ids: vec![2],
                item_ids: vec![],
            },
        );

        // Blacksmith
        let mut smith_exits = HashMap::new();
        smith_exits.insert(Direction::West, 1);

        self.rooms.insert(3, Room {
            id: 3,
            name: "Blacksmith Shop".to_string(),
            description: "The rhythmic clang of hammer on anvil fills this dimly lit shop. Weapons and armor line the walls.".to_string(),
            exits: smith_exits,
            npc_ids: vec![3],
            item_ids: vec![1, 2],
        });

        // Market
        let mut market_exits = HashMap::new();
        market_exits.insert(Direction::North, 1);

        self.rooms.insert(
            4,
            Room {
                id: 4,
                name: "Market".to_string(),
                description:
                    "A lively market with colorful stalls. Merchants call out to passersby."
                        .to_string(),
                exits: market_exits,
                npc_ids: vec![],
                item_ids: vec![3, 4],
            },
        );

        // Goblin Cave
        let mut cave_exits = HashMap::new();
        cave_exits.insert(Direction::South, 2);

        self.rooms.insert(5, Room {
            id: 5,
            name: "Goblin Cave".to_string(),
            description: "A dark, damp cave. The sound of dripping water echoes through the darkness. Something moves in the shadows.".to_string(),
            exits: cave_exits,
            npc_ids: vec![4],
            item_ids: vec![5],
        });

        // NPCs
        self.npcs.insert(
            1,
            Npc {
                id: 1,
                name: "Town Guard".to_string(),
                description: "A stern-looking guard standing at attention.".to_string(),
                room_id: 1,
                hp: 100,
                max_hp: 100,
                level: 5,
                attack: 12,
                defense: 10,
            },
        );

        self.npcs.insert(
            2,
            Npc {
                id: 2,
                name: "Forest Wolf".to_string(),
                description: "A gray wolf with piercing yellow eyes.".to_string(),
                room_id: 2,
                hp: 50,
                max_hp: 50,
                level: 2,
                attack: 8,
                defense: 4,
            },
        );

        self.npcs.insert(
            3,
            Npc {
                id: 3,
                name: "Blacksmith Garen".to_string(),
                description: "A burly man with soot-stained arms, hammering a blade.".to_string(),
                room_id: 3,
                hp: 120,
                max_hp: 120,
                level: 4,
                attack: 10,
                defense: 8,
            },
        );

        self.npcs.insert(
            4,
            Npc {
                id: 4,
                name: "Goblin".to_string(),
                description: "A small, green creature with sharp teeth and a rusty dagger."
                    .to_string(),
                room_id: 5,
                hp: 30,
                max_hp: 30,
                level: 1,
                attack: 5,
                defense: 2,
            },
        );
    }

    pub fn get_room(&self, room_id: u32) -> Option<&Room> {
        self.rooms.get(&room_id)
    }

    pub fn get_npc(&self, npc_id: u64) -> Option<&Npc> {
        self.npcs.get(&npc_id)
    }

    pub fn get_npc_mut(&mut self, npc_id: u64) -> Option<&mut Npc> {
        self.npcs.get_mut(&npc_id)
    }

    pub fn find_npc_in_room(&self, room_id: u32, name: &str) -> Option<&Npc> {
        self.npcs.values().find(|npc| {
            npc.room_id == room_id && npc.name.to_lowercase().contains(&name.to_lowercase())
        })
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    /// Insert an NPC and register it in its room.
    ///
    /// `Npc::room_id` and `Room::npc_ids` are two representations of the
    /// same fact, and `look_room()` reads the latter - so anything that
    /// changes where an NPC is must update both. That's why callers go
    /// through these methods rather than touching `npcs` directly.
    pub fn add_npc(&mut self, npc: Npc) {
        if let Some(room) = self.rooms.get_mut(&npc.room_id) {
            if !room.npc_ids.contains(&npc.id) {
                room.npc_ids.push(npc.id);
            }
        }
        self.npcs.insert(npc.id, npc);
    }

    /// Move an NPC to another room, keeping both representations in sync.
    /// Returns the room it came from, or `None` if the NPC or the
    /// destination doesn't exist (in which case nothing is changed).
    pub fn move_npc(&mut self, npc_id: u64, to_room_id: u32) -> Option<u32> {
        if !self.rooms.contains_key(&to_room_id) {
            return None;
        }
        let from_room_id = self.npcs.get(&npc_id)?.room_id;
        if from_room_id == to_room_id {
            return Some(from_room_id);
        }

        if let Some(room) = self.rooms.get_mut(&from_room_id) {
            room.npc_ids.retain(|id| *id != npc_id);
        }
        if let Some(room) = self.rooms.get_mut(&to_room_id) {
            if !room.npc_ids.contains(&npc_id) {
                room.npc_ids.push(npc_id);
            }
        }
        if let Some(npc) = self.npcs.get_mut(&npc_id) {
            npc.room_id = to_room_id;
        }
        Some(from_room_id)
    }

    /// Remove an NPC entirely. Returns the room it was in.
    pub fn remove_npc(&mut self, npc_id: u64) -> Option<u32> {
        let npc = self.npcs.remove(&npc_id)?;
        if let Some(room) = self.rooms.get_mut(&npc.room_id) {
            room.npc_ids.retain(|id| *id != npc_id);
        }
        Some(npc.room_id)
    }

    /// Where an NPC could go from its current room, as (direction, room_id).
    pub fn exits_from(&self, room_id: u32) -> Vec<(Direction, u32)> {
        self.rooms
            .get(&room_id)
            .map(|room| room.exits.iter().map(|(d, r)| (*d, *r)).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_npc(id: u64, room_id: u32) -> Npc {
        Npc {
            id,
            name: format!("NPC {}", id),
            description: String::new(),
            room_id,
            hp: 20,
            max_hp: 20,
            level: 1,
            attack: 5,
            defense: 2,
        }
    }

    /// `Npc.room_id` and `Room.npc_ids` are two representations of the same
    /// fact, and `look_room()` reads the latter. If they ever disagree a
    /// room either shows a ghost NPC or hides a present one - so every one
    /// of these tests asserts BOTH sides.
    fn assert_consistent(world: &World, npc_id: u64, expected_room: Option<u32>) {
        match expected_room {
            Some(room_id) => {
                let npc = world.get_npc(npc_id).expect("npc should exist");
                assert_eq!(npc.room_id, room_id, "npc.room_id");
                assert!(
                    world.get_room(room_id).unwrap().npc_ids.contains(&npc_id),
                    "room {} should list npc {}",
                    room_id,
                    npc_id
                );
            }
            None => {
                assert!(world.get_npc(npc_id).is_none(), "npc should be gone");
            }
        }
        // Whatever the expectation, the id must appear in at most one room.
        let listings: Vec<u32> = world
            .rooms
            .values()
            .filter(|r| r.npc_ids.contains(&npc_id))
            .map(|r| r.id)
            .collect();
        assert!(
            listings.len() <= 1,
            "npc {} is listed in multiple rooms: {:?}",
            npc_id,
            listings
        );
        if let Some(room_id) = expected_room {
            assert_eq!(listings, vec![room_id]);
        } else {
            assert!(
                listings.is_empty(),
                "removed npc still listed in {:?}",
                listings
            );
        }
    }

    #[test]
    fn seeded_world_is_internally_consistent() {
        // The hardcoded NPCs in initialize() must satisfy the same
        // invariant as dynamically added ones.
        let world = World::new();
        for id in world.npcs.keys() {
            assert_consistent(&world, *id, Some(world.get_npc(*id).unwrap().room_id));
        }
    }

    #[test]
    fn add_npc_registers_it_in_its_room() {
        let mut world = World::new();
        world.add_npc(make_npc(500, 2));
        assert_consistent(&world, 500, Some(2));
    }

    #[test]
    fn add_npc_is_idempotent_for_the_same_id() {
        let mut world = World::new();
        world.add_npc(make_npc(500, 2));
        world.add_npc(make_npc(500, 2));
        // Not listed twice - assert_consistent's uniqueness check would
        // pass either way, so check the count directly.
        let count = world
            .get_room(2)
            .unwrap()
            .npc_ids
            .iter()
            .filter(|id| **id == 500)
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn move_npc_updates_both_rooms_and_the_npc() {
        let mut world = World::new();
        world.add_npc(make_npc(500, 1));

        let from = world.move_npc(500, 2).expect("move should succeed");

        assert_eq!(from, 1);
        assert_consistent(&world, 500, Some(2));
        assert!(
            !world.get_room(1).unwrap().npc_ids.contains(&500),
            "old room must drop the npc"
        );
    }

    #[test]
    fn move_npc_to_an_unknown_room_changes_nothing() {
        let mut world = World::new();
        world.add_npc(make_npc(500, 1));

        assert!(world.move_npc(500, 9999).is_none());
        assert_consistent(&world, 500, Some(1));
    }

    #[test]
    fn move_npc_that_does_not_exist_is_none() {
        let mut world = World::new();
        assert!(world.move_npc(12345, 2).is_none());
    }

    #[test]
    fn move_npc_to_its_current_room_is_a_noop() {
        let mut world = World::new();
        world.add_npc(make_npc(500, 1));
        assert_eq!(world.move_npc(500, 1), Some(1));
        assert_consistent(&world, 500, Some(1));
    }

    #[test]
    fn remove_npc_clears_it_from_its_room() {
        let mut world = World::new();
        world.add_npc(make_npc(500, 3));

        assert_eq!(world.remove_npc(500), Some(3));
        assert_consistent(&world, 500, None);
        assert!(world.remove_npc(500).is_none(), "second remove is None");
    }

    #[test]
    fn removing_a_seeded_npc_also_clears_its_room() {
        // The Goblin (id 4) lives in room 5 per initialize().
        let mut world = World::new();
        let room = world.get_npc(4).unwrap().room_id;
        assert_eq!(world.remove_npc(4), Some(room));
        assert_consistent(&world, 4, None);
    }

    #[test]
    fn exits_from_matches_the_rooms_exit_table() {
        let world = World::new();
        let exits = world.exits_from(1);
        assert_eq!(exits.len(), world.get_room(1).unwrap().exits.len());
        for (direction, room_id) in exits {
            assert_eq!(
                world.get_room(1).unwrap().exits.get(&direction),
                Some(&room_id)
            );
        }
        assert!(
            world.exits_from(9999).is_empty(),
            "unknown room has no exits"
        );
    }

    #[test]
    fn find_npc_in_room_is_case_insensitive_and_room_scoped() {
        let world = World::new();
        // Forest Wolf is in room 2.
        assert!(world.find_npc_in_room(2, "wolf").is_some());
        assert!(world.find_npc_in_room(2, "WOLF").is_some());
        assert!(
            world.find_npc_in_room(1, "wolf").is_none(),
            "must not find an npc from another room"
        );
    }

    #[test]
    fn direction_round_trips_and_opposites_pair_up() {
        for (text, expected) in [
            ("north", Direction::North),
            ("N", Direction::North),
            ("down", Direction::Down),
        ] {
            assert_eq!(Direction::from_str(text), Some(expected));
        }
        assert_eq!(Direction::from_str("sideways"), None);

        for d in [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
            Direction::Up,
            Direction::Down,
        ] {
            assert_eq!(d.opposite().opposite(), d);
            assert_ne!(d.opposite(), d);
        }
    }
}
