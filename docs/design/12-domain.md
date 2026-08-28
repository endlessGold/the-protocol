# 12 - Domain Layer

## Overview

The Domain Layer contains pure game logic entities and rules. It has NO dependencies on infrastructure, networking, or the Runtime.

## Design Principles

1. **No infrastructure dependencies**: Domain does not know about TCP, Redis, PostgreSQL, WASM
2. **Pure business logic**: Entities contain behavior, not just data
3. **Language-agnostic**: Same domain concepts apply across plugins, API, and runtime
4. **Testable**: Domain logic can be unit tested without any infrastructure

## Domain Structure

```
/domain
    /character     - Character entity and rules
    /combat        - Combat system entities
    /inventory     - Inventory and items
    /world         - World, rooms, movement
    /auction       - Auction house
    /guild         - Guild system
    /event         - Domain events
    /command       - Domain commands
```

## Core Entities

### Character

```rust
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
    pub position: Position,
    pub inventory: Inventory,
    pub equipment: Equipment,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CharacterClass {
    Warrior,
    Mage,
    Rogue,
    Cleric,
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
pub struct Position {
    pub room_id: u32,
    pub x: f64,
    pub y: f64,
}

impl Character {
    pub fn new(name: String, class: CharacterClass) -> Self {
        let base_stats = class.base_stats();
        Self {
            id: 0,
            name,
            class,
            level: 1,
            experience: 0,
            hp: base_stats.max_hp,
            max_hp: base_stats.max_hp,
            mp: base_stats.max_mp,
            max_mp: base_stats.max_mp,
            stats: base_stats,
            position: Position { room_id: 1, x: 0.0, y: 0.0 },
            inventory: Inventory::new(),
            equipment: Equipment::new(),
            created_at: Utc::now(),
        }
    }

    pub fn take_damage(&mut self, amount: u32) -> u32 {
        let actual_damage = amount.min(self.hp);
        self.hp -= actual_damage;
        actual_damage
    }

    pub fn heal(&mut self, amount: u32) -> u32 {
        let actual_heal = amount.min(self.max_hp - self.hp);
        self.hp += actual_heal;
        actual_heal
    }

    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }

    pub fn xp_for_next_level(&self) -> u64 {
        (self.level as u64) * 1000
    }

    pub fn gain_experience(&mut self, xp: u64) -> Vec<DomainEvent> {
        let mut events = Vec::new();
        self.experience += xp;

        while self.experience >= self.xp_for_next_level() {
            self.experience -= self.xp_for_next_level();
            self.level += 1;
            events.push(DomainEvent::LevelUp {
                character_id: self.id,
                new_level: self.level,
            });
        }

        events
    }
}

impl CharacterClass {
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
```

### Combat

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Combat {
    pub id: u64,
    pub attacker_id: u64,
    pub target_id: u64,
    pub state: CombatState,
    pub turn: u32,
    pub log: Vec<CombatAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CombatState {
    WaitingForAttacker,
    WaitingForTarget,
    InProgress,
    Finished { winner_id: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatAction {
    pub actor_id: u64,
    pub action_type: CombatActionType,
    pub damage: Option<u32>,
    pub healing: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CombatActionType {
    Attack,
    Defend,
    UseItem,
    Flee,
}

impl Combat {
    pub fn new(attacker_id: u64, target_id: u64) -> Self {
        Self {
            id: 0,
            attacker_id,
            target_id,
            state: CombatState::WaitingForAttacker,
            turn: 1,
            log: Vec::new(),
        }
    }

    pub fn calculate_damage(attacker: &Character, target: &Character, weapon: Option<&Item>) -> u32 {
        let base_damage = attacker.stats.strength as f64;
        let weapon_bonus = weapon.map(|w| w.damage_bonus as f64).unwrap_or(0.0);
        let defense = target.stats.constitution as f64 * 0.5;

        let raw_damage = (base_damage + weapon_bonus - defense).max(1.0);

        // Add some randomness (±20%)
        let mut rng = thread_rng();
        let variance = raw_damage * 0.2;
        let final_damage = raw_damage + rng.gen_range(-variance..variance);

        (final_damage.max(1.0) as u32)
    }

    pub fn process_action(&mut self, action: CombatAction, characters: &mut HashMap<u64, Character>) -> Vec<DomainEvent> {
        let mut events = Vec::new();

        match action.action_type {
            CombatActionType::Attack => {
                if let (Some(attacker), Some(target)) = (
                    characters.get_mut(&self.attacker_id),
                    characters.get_mut(&self.target_id),
                ) {
                    let damage = Self::calculate_damage(attacker, target, None);
                    target.take_damage(damage);

                    self.log.push(CombatAction {
                        actor_id: self.attacker_id,
                        action_type: CombatActionType::Attack,
                        damage: Some(damage),
                        healing: None,
                        message: format!("{} attacks {} for {} damage!", attacker.name, target.name, damage),
                    });

                    if !target.is_alive() {
                        self.state = CombatState::Finished { winner_id: self.attacker_id };
                        events.push(DomainEvent::CombatEnded {
                            combat_id: self.id,
                            winner_id: self.attacker_id,
                            loser_id: self.target_id,
                        });
                    }
                }
            }
            CombatActionType::Defend => {
                // Reduce damage taken next turn
            }
            CombatActionType::Flee => {
                // Chance to escape
            }
            _ => {}
        }

        self.turn += 1;
        events
    }
}
```

### Inventory

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    pub items: Vec<ItemStack>,
    pub capacity: usize,
    pub gold: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemStack {
    pub item_id: u32,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub item_type: ItemType,
    pub rarity: Rarity,
    pub damage_bonus: u32,
    pub defense_bonus: u32,
    pub value: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ItemType {
    Weapon,
    Armor,
    Consumable,
    Quest,
    Material,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            capacity: 20,
            gold: 0,
        }
    }

    pub fn add_item(&mut self, item_id: u32, quantity: u32) -> Result<(), InventoryError> {
        // Check if item already exists in inventory
        if let Some(stack) = self.items.iter_mut().find(|s| s.item_id == item_id) {
            stack.quantity += quantity;
            return Ok(());
        }

        // Check capacity
        if self.items.len() >= self.capacity {
            return Err(InventoryError::Full);
        }

        self.items.push(ItemStack { item_id, quantity });
        Ok(())
    }

    pub fn remove_item(&mut self, item_id: u32, quantity: u32) -> Result<(), InventoryError> {
        if let Some(stack) = self.items.iter_mut().find(|s| s.item_id == item_id) {
            if stack.quantity < quantity {
                return Err(InventoryError::InsufficientQuantity);
            }
            stack.quantity -= quantity;
            if stack.quantity == 0 {
                self.items.retain(|s| s.item_id != item_id);
            }
            Ok(())
        } else {
            Err(InventoryError::ItemNotFound)
        }
    }

    pub fn has_item(&self, item_id: u32, quantity: u32) -> bool {
        self.items.iter()
            .find(|s| s.item_id == item_id)
            .map(|s| s.quantity >= quantity)
            .unwrap_or(false)
    }
}
```

### World

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub exits: HashMap<Direction, u32>,
    pub entities: Vec<EntityRef>,
    pub items: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    North,
    South,
    East,
    West,
    Up,
    Down,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntityRef {
    Player(u64),
    NPC(u64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NPC {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub room_id: u32,
    pub hp: u32,
    pub max_hp: u32,
    pub dialogue: Option<String>,
    pub loot_table: Vec<LootEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LootEntry {
    pub item_id: u32,
    pub chance: f64,  // 0.0 - 1.0
    pub quantity: u32,
}
```

## Domain Events

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEvent {
    // Character events
    CharacterCreated {
        character_id: u64,
        name: String,
        class: CharacterClass,
    },
    LevelUp {
        character_id: u64,
        new_level: u32,
    },

    // Combat events
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

    // World events
    PlayerEnteredRoom {
        player_id: u64,
        room_id: u32,
    },
    PlayerLeftRoom {
        player_id: u64,
        room_id: u32,
    },

    // Inventory events
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

    // Auction events
    AuctionCreated {
        auction_id: u64,
        seller_id: u64,
        item_id: u32,
        price: u64,
    },
    AuctionSold {
        auction_id: u64,
        buyer_id: u64,
        seller_id: u64,
        price: u64,
    },
}
```

## Domain Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("Character not found: {0}")]
    CharacterNotFound(u64),

    #[error("Item not found: {0}")]
    ItemNotFound(u32),

    #[error("Insufficient gold: need {needed}, have {have}")]
    InsufficientGold { needed: u64, have: u64 },

    #[error("Inventory full")]
    InventoryFull,

    #[error("Cannot move in that direction")]
    NoExit,

    #[error("Target is already dead")]
    TargetDead,

    #[error("Cannot attack yourself")]
    SelfAttack,
}
```

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Entity IDs | u64 | Simple, sufficient for game entities |
| Serialization | Serde + MessagePack | Cross-language, compact |
| Events | Enum-based | Type safety, exhaustive matching |
| Domain rules | In entities | Keep behavior close to data |
| Dependencies | None | Pure domain, no infrastructure |

## References

- [13-application.md](13-application.md) - Application layer using domain
- [04-plugin-system.md](04-plugin-system.md) - Plugins implement domain logic
- [12-domain.md](12-domain.md) - This document
