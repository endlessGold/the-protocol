# 13 - Application Layer

## Overview

The Application Layer sits between the Domain Layer and the Infrastructure Layer. It orchestrates domain entities to implement use cases, and is used by both the Runtime (via plugins) and the Web API.

## Layer Position

```
┌─────────────────────────────────────────┐
│  Transport (TCP/UDP/HTTP/WebSocket)     │
├─────────────────────────────────────────┤
│  Protocol (Codec, Routing)              │
├─────────────────────────────────────────┤
│  Session                                │
├─────────────────────────────────────────┤
│  ┌─────────────────────────────────────┐│
│  │  Application Layer                  ││
│  │  ┌──────────────┐ ┌──────────────┐ ││
│  │  │ CharacterSvc │ │ CombatSvc    │ ││
│  │  │ InventorySvc │ │ AuctionSvc   │ ││
│  │  │ WorldSvc     │ │ GuildSvc     │ ││
│  │  └──────────────┘ └──────────────┘ ││
│  └─────────────────────────────────────┘│
├─────────────────────────────────────────┤
│  ┌─────────────────────────────────────┐│
│  │  Domain Layer                       ││
│  │  Character, Combat, Inventory, etc. ││
│  └─────────────────────────────────────┘│
├─────────────────────────────────────────┤
│  Repository (Database abstraction)      │
└─────────────────────────────────────────┘
```

## Application Services

### CharacterService

```rust
#[async_trait]
pub trait CharacterService: Send + Sync {
    async fn create_character(&self, account_id: u64, name: String, class: CharacterClass) -> Result<Character>;
    async fn get_character(&self, character_id: u64) -> Result<Character>;
    async fn get_characters_by_account(&self, account_id: u64) -> Result<Vec<Character>>;
    async fn update_character(&self, character: &Character) -> Result<()>;
    async fn delete_character(&self, character_id: u64) -> Result<()>;
    async fn gain_experience(&self, character_id: u64, xp: u64) -> Result<Vec<DomainEvent>>;
}

pub struct PostgresCharacterService {
    repo: Arc<dyn CharacterRepository>,
}

#[async_trait]
impl CharacterService for PostgresCharacterService {
    async fn create_character(&self, account_id: u64, name: String, class: CharacterClass) -> Result<Character> {
        // Validate name
        if name.len() < 3 || name.len() > 20 {
            return Err(ApplicationError::InvalidCharacterName(name));
        }

        // Check if name is taken
        if self.repo.find_by_name(&name).await?.is_some() {
            return Err(ApplicationError::CharacterNameTaken(name));
        }

        let character = Character::new(name, class);
        let saved = self.repo.save(&character).await?;
        Ok(saved)
    }

    async fn gain_experience(&self, character_id: u64, xp: u64) -> Result<Vec<DomainEvent>> {
        let mut character = self.repo.find_by_id(character_id).await?
            .ok_or(ApplicationError::CharacterNotFound(character_id))?;

        let events = character.gain_experience(xp);
        self.repo.save(&character).await?;

        Ok(events)
    }
}
```

### CombatService

```rust
#[async_trait]
pub trait CombatService: Send + Sync {
    async fn start_combat(&self, attacker_id: u64, target_id: u64) -> Result<Combat>;
    async fn execute_action(&self, combat_id: u64, action: CombatAction) -> Result<CombatResult>;
    async fn get_combat(&self, combat_id: u64) -> Result<Combat>;
    async fn end_combat(&self, combat_id: u64) -> Result<CombatResult>;
}

pub struct DefaultCombatService {
    combat_repo: Arc<dyn CombatRepository>,
    character_repo: Arc<dyn CharacterRepository>,
}

#[async_trait]
impl CombatService for DefaultCombatService {
    async fn start_combat(&self, attacker_id: u64, target_id: u64) -> Result<Combat> {
        let attacker = self.character_repo.find_by_id(attacker_id).await?
            .ok_or(ApplicationError::CharacterNotFound(attacker_id))?;
        let target = self.character_repo.find_by_id(target_id).await?
            .ok_or(ApplicationError::CharacterNotFound(target_id))?;

        if attacker.room_id != target.room_id {
            return Err(ApplicationError::TargetNotInSameRoom);
        }

        if !attacker.is_alive() || !target.is_alive() {
            return Err(ApplicationError::CombatantNotAlive);
        }

        let combat = Combat::new(attacker_id, target_id);
        let saved = self.combat_repo.save(&combat).await?;

        Ok(saved)
    }

    async fn execute_action(&self, combat_id: u64, action: CombatAction) -> Result<CombatResult> {
        let mut combat = self.combat_repo.find_by_id(combat_id).await?
            .ok_or(ApplicationError::CombatNotFound(combat_id))?;

        // Get characters
        let mut characters = self.character_repo
            .find_many(&[combat.attacker_id, combat.target_id]).await?;

        // Process action
        let events = combat.process_action(action, &mut characters);

        // Save changes
        for character in characters.values() {
            self.character_repo.save(character).await?;
        }
        self.combat_repo.save(&combat).await?;

        Ok(CombatResult { combat, events })
    }
}
```

### InventoryService

```rust
#[async_trait]
pub trait InventoryService: Send + Sync {
    async fn get_inventory(&self, player_id: u64) -> Result<Inventory>;
    async fn add_item(&self, player_id: u64, item_id: u32, quantity: u32) -> Result<DomainEvent>;
    async fn remove_item(&self, player_id: u64, item_id: u32, quantity: u32) -> Result<DomainEvent>;
    async fn has_item(&self, player_id: u64, item_id: u32, quantity: u32) -> Result<bool>;
    async fn use_item(&self, player_id: u64, item_id: u32) -> Result<DomainEvent>;
}

pub struct DefaultInventoryService {
    inventory_repo: Arc<dyn InventoryRepository>,
    item_repo: Arc<dyn ItemRepository>,
}

#[async_trait]
impl InventoryService for DefaultInventoryService {
    async fn add_item(&self, player_id: u64, item_id: u32, quantity: u32) -> Result<DomainEvent> {
        let mut inventory = self.inventory_repo.find_by_player(player_id).await?
            .ok_or(ApplicationError::InventoryNotFound(player_id))?;

        // Check if item exists
        let _item = self.item_repo.find_by_id(item_id).await?
            .ok_or(ApplicationError::ItemNotFound(item_id))?;

        inventory.add_item(item_id, quantity)?;
        self.inventory_repo.save(&inventory).await?;

        Ok(DomainEvent::ItemAcquired { player_id, item_id, quantity })
    }
}
```

### AuctionService

```rust
#[async_trait]
pub trait AuctionService: Send + Sync {
    async fn create_listing(&self, seller_id: u64, item_id: u32, quantity: u32, price: u64) -> Result<AuctionListing>;
    async fn buy_item(&self, listing_id: u64, buyer_id: u64) -> Result<AuctionSale>;
    async fn cancel_listing(&self, listing_id: u64, seller_id: u64) -> Result<()>;
    async fn search_listings(&self, query: &AuctionSearchQuery) -> Result<Vec<AuctionListing>>;
    async fn get_listing(&self, listing_id: u64) -> Result<AuctionListing>;
}

pub struct DefaultAuctionService {
    auction_repo: Arc<dyn AuctionRepository>,
    inventory_service: Arc<dyn InventoryService>,
}

#[async_trait]
impl AuctionService for DefaultAuctionService {
    async fn create_listing(&self, seller_id: u64, item_id: u32, quantity: u32, price: u64) -> Result<AuctionListing> {
        // Verify seller has item
        if !self.inventory_service.has_item(seller_id, item_id, quantity).await? {
            return Err(ApplicationError::InsufficientItems);
        }

        // Remove item from seller's inventory
        self.inventory_service.remove_item(seller_id, item_id, quantity).await?;

        let listing = AuctionListing {
            id: 0,
            seller_id,
            item_id,
            quantity,
            price,
            status: AuctionStatus::Active,
            created_at: Utc::now(),
        };

        let saved = self.auction_repo.save(&listing).await?;
        Ok(saved)
    }

    async fn buy_item(&self, listing_id: u64, buyer_id: u64) -> Result<AuctionSale> {
        let listing = self.auction_repo.find_by_id(listing_id).await?
            .ok_or(ApplicationError::AuctionNotFound(listing_id))?;

        if listing.status != AuctionStatus::Active {
            return Err(ApplicationError::AuctionNotActive);
        }

        // Add item to buyer's inventory
        self.inventory_service.add_item(buyer_id, listing.item_id, listing.quantity).await?;

        // Mark listing as sold
        let mut listing = listing;
        listing.status = AuctionStatus::Sold;
        self.auction_repo.save(&listing).await?;

        Ok(AuctionSale {
            listing_id,
            buyer_id,
            seller_id: listing.seller_id,
            price: listing.price,
        })
    }
}
```

### WorldService

```rust
#[async_trait]
pub trait WorldService: Send + Sync {
    async fn get_room(&self, room_id: u32) -> Result<Room>;
    async fn move_player(&self, player_id: u64, direction: Direction) -> Result<MoveResult>;
    async fn get_room_players(&self, room_id: u32) -> Result<Vec<PlayerSummary>>;
    async fn get_room_npcs(&self, room_id: u32) -> Result<Vec<NPC>>;
}

pub struct DefaultWorldService {
    world_repo: Arc<dyn WorldRepository>,
    character_repo: Arc<dyn CharacterRepository>,
}

#[async_trait]
impl WorldService for DefaultWorldService {
    async fn move_player(&self, player_id: u64, direction: Direction) -> Result<MoveResult> {
        let mut character = self.character_repo.find_by_id(player_id).await?
            .ok_or(ApplicationError::CharacterNotFound(player_id))?;

        let current_room = self.world_repo.get_room(character.room_id).await?
            .ok_or(ApplicationError::RoomNotFound(character.room_id))?;

        let new_room_id = current_room.exits.get(&direction)
            .ok_or(ApplicationError::NoExit)?
            .clone();

        let new_room = self.world_repo.get_room(new_room_id).await?
            .ok_or(ApplicationError::RoomNotFound(new_room_id))?;

        // Update character position
        let old_room_id = character.room_id;
        character.position.room_id = new_room_id;
        self.character_repo.save(&character).await?;

        // Update room entities
        self.world_repo.remove_entity(old_room_id, EntityRef::Player(player_id)).await?;
        self.world_repo.add_entity(new_room_id, EntityRef::Player(player_id)).await?;

        Ok(MoveResult {
            from_room: current_room,
            to_room: new_room,
            events: vec![
                DomainEvent::PlayerLeftRoom { player_id, room_id: old_room_id },
                DomainEvent::PlayerEnteredRoom { player_id, room_id: new_room_id },
            ],
        })
    }
}
```

## Command/Event Bridge

Application services are invoked through the Command/Event system:

```rust
// Command → Service → Domain → Event
pub struct ApplicationCommandHandler {
    character_service: Arc<dyn CharacterService>,
    combat_service: Arc<dyn CombatService>,
    inventory_service: Arc<dyn InventoryService>,
    world_service: Arc<dyn WorldService>,
}

#[async_trait]
impl CommandHandler for ApplicationCommandHandler {
    async fn handle(&self, command: Command, session: &Session) -> Result<CommandResponse> {
        match command.command_type.as_str() {
            "login" => self.handle_login(command, session).await,
            "create_character" => self.handle_create_character(command, session).await,
            "look" => self.handle_look(command, session).await,
            "move" => self.handle_move(command, session).await,
            "attack" => self.handle_attack(command, session).await,
            "inventory" => self.handle_inventory(command, session).await,
            _ => Err(ApplicationError::UnknownCommand(command.command_type)),
        }
    }
}
```

## Web API ↔ Runtime Shared Logic

```
Web API (HTTPS)                    Game Runtime (TCP)
      │                                  │
      ▼                                  ▼
┌─────────────┐                   ┌─────────────┐
│ CharacterSvc│                   │ CharacterSvc│  ← Same service
│ CombatSvc   │                   │ CombatSvc   │
│ InventorySvc│                   │ InventorySvc│
└─────────────┘                   └─────────────┘
      │                                  │
      ▼                                  ▼
┌─────────────┐                   ┌─────────────┐
│ PostgreSQL  │                   │ PostgreSQL  │  ← Same DB
└─────────────┘                   └─────────────┘
```

Both the Web API and the Runtime use the same Application Services. This ensures consistent game rules regardless of access path.

## References

- [12-domain.md](12-domain.md) - Domain entities
- [04-plugin-system.md](04-plugin-system.md) - Plugins call application services
- [05-plugin-api.md](05-plugin-api.md) - Plugin host functions
