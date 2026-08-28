use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CharacterClass {
    Warrior,
    Mage,
    Rogue,
    Cleric,
}

impl CharacterClass {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "warrior" => Some(Self::Warrior),
            "mage" => Some(Self::Mage),
            "rogue" => Some(Self::Rogue),
            "cleric" => Some(Self::Cleric),
            _ => None,
        }
    }

    pub fn base_stats(&self) -> Stats {
        match self {
            CharacterClass::Warrior => Stats {
                strength: 15,
                dexterity: 10,
                intelligence: 8,
                wisdom: 8,
                constitution: 14,
            },
            CharacterClass::Mage => Stats {
                strength: 8,
                dexterity: 10,
                intelligence: 15,
                wisdom: 12,
                constitution: 10,
            },
            CharacterClass::Rogue => Stats {
                strength: 10,
                dexterity: 15,
                intelligence: 10,
                wisdom: 8,
                constitution: 12,
            },
            CharacterClass::Cleric => Stats {
                strength: 10,
                dexterity: 8,
                intelligence: 12,
                wisdom: 15,
                constitution: 12,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub strength: u32,
    pub dexterity: u32,
    pub intelligence: u32,
    pub wisdom: u32,
    pub constitution: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Character {
    pub id: u64,
    pub name: String,
    pub class: CharacterClass,
    pub level: u32,
    pub experience: u64,
    pub hp: u32,
    pub max_hp: u32,
    pub mp: u32,
    pub max_mp: u32,
    pub stats: Stats,
    pub room_id: u32,
    pub inventory: Inventory,
}

use crate::inventory::Inventory;

impl Character {
    pub fn new(name: String, class: CharacterClass) -> Self {
        let base_stats = class.base_stats();
        let max_hp = 50 + (base_stats.constitution * 2);
        Self {
            id: 0,
            name,
            class,
            level: 1,
            experience: 0,
            hp: max_hp,
            max_hp,
            mp: 20 + (base_stats.wisdom),
            max_mp: 20 + (base_stats.wisdom),
            stats: base_stats,
            room_id: 1,
            inventory: Inventory::new(),
        }
    }

    pub fn take_damage(&mut self, amount: u32) -> u32 {
        let actual = amount.min(self.hp);
        self.hp -= actual;
        actual
    }

    pub fn heal(&mut self, amount: u32) -> u32 {
        let actual = amount.min(self.max_hp - self.hp);
        self.hp += actual;
        actual
    }

    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }

    pub fn xp_for_next_level(&self) -> u64 {
        (self.level as u64) * 1000
    }

    pub fn gain_experience(&mut self, xp: u64) -> Vec<crate::event::DomainEvent> {
        let mut events = Vec::new();
        self.experience += xp;

        while self.experience >= self.xp_for_next_level() {
            self.experience -= self.xp_for_next_level();
            self.level += 1;
            self.max_hp += 10;
            self.hp = self.max_hp;
            events.push(crate::event::DomainEvent::LevelUp {
                character_id: self.id,
                new_level: self.level,
            });
        }

        events
    }
}

impl crate::combatant::Combatant for Character {
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
        Character::take_damage(self, amount)
    }

    fn is_alive(&self) -> bool {
        Character::is_alive(self)
    }

    fn offense(&self) -> u32 {
        self.stats.strength
    }

    fn defense(&self) -> u32 {
        self.stats.constitution
    }

    fn grant_experience(&mut self, xp: u64) -> Vec<crate::event::DomainEvent> {
        self.gain_experience(xp)
    }
}
