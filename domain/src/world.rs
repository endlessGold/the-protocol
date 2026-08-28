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

        self.rooms.insert(2, Room {
            id: 2,
            name: "Forest Path".to_string(),
            description: "A winding path through a dense forest. Birds chirp in the canopy above.".to_string(),
            exits: forest_exits,
            npc_ids: vec![2],
            item_ids: vec![],
        });

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

        self.rooms.insert(4, Room {
            id: 4,
            name: "Market".to_string(),
            description: "A lively market with colorful stalls. Merchants call out to passersby.".to_string(),
            exits: market_exits,
            npc_ids: vec![],
            item_ids: vec![3, 4],
        });

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
        self.npcs.insert(1, Npc {
            id: 1,
            name: "Town Guard".to_string(),
            description: "A stern-looking guard standing at attention.".to_string(),
            room_id: 1,
            hp: 100,
            max_hp: 100,
        });

        self.npcs.insert(2, Npc {
            id: 2,
            name: "Forest Wolf".to_string(),
            description: "A gray wolf with piercing yellow eyes.".to_string(),
            room_id: 2,
            hp: 50,
            max_hp: 50,
        });

        self.npcs.insert(3, Npc {
            id: 3,
            name: "Blacksmith Garen".to_string(),
            description: "A burly man with soot-stained arms, hammering a blade.".to_string(),
            room_id: 3,
            hp: 120,
            max_hp: 120,
        });

        self.npcs.insert(4, Npc {
            id: 4,
            name: "Goblin".to_string(),
            description: "A small, green creature with sharp teeth and a rusty dagger.".to_string(),
            room_id: 5,
            hp: 30,
            max_hp: 30,
        });
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
        self.npcs.values()
            .find(|npc| npc.room_id == room_id && npc.name.to_lowercase().contains(&name.to_lowercase()))
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}
