# 15 - Database

## Overview

PostgreSQL is the primary database. Domain entities never access the database directly - Repository abstractions provide the interface.

## Architecture

```
┌─────────────────────────────────────────┐
│  Application Layer                      │
│  (CharacterService, CombatService, etc.)│
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│  Repository Layer (trait)               │
│  CharacterRepository                    │
│  InventoryRepository                    │
│  AuctionRepository                      │
│  WorldRepository                        │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│  PostgreSQL Implementation              │
│  (sqlx - async, type-safe)             │
└──────────────────┬──────────────────────┘
                   │
              ┌────▼────┐
              │PostgreSQL│
              └─────────┘
```

## Repository Traits

```rust
#[async_trait]
pub trait CharacterRepository: Send + Sync {
    async fn find_by_id(&self, id: u64) -> Result<Option<Character>>;
    async fn find_by_name(&self, name: &str) -> Result<Option<Character>>;
    async fn find_by_account(&self, account_id: u64) -> Result<Vec<Character>>;
    async fn save(&self, character: &Character) -> Result<Character>;
    async fn delete(&self, id: u64) -> Result<()>;
}

#[async_trait]
pub trait InventoryRepository: Send + Sync {
    async fn find_by_player(&self, player_id: u64) -> Result<Option<Inventory>>;
    async fn save(&self, inventory: &Inventory) -> Result<()>;
}

#[async_trait]
pub trait AuctionRepository: Send + Sync {
    async fn find_by_id(&self, id: u64) -> Result<Option<AuctionListing>>;
    async fn find_active(&self, query: &AuctionSearchQuery) -> Result<Vec<AuctionListing>>;
    async fn save(&self, listing: &AuctionListing) -> Result<AuctionListing>;
    async fn delete(&self, id: u64) -> Result<()>;
}

#[async_trait]
pub trait WorldRepository: Send + Sync {
    async fn get_room(&self, room_id: u32) -> Result<Option<Room>>;
    async fn get_npc(&self, npc_id: u64) -> Result<Option<NPC>>;
    async fn add_entity(&self, room_id: u32, entity: EntityRef) -> Result<()>;
    async fn remove_entity(&self, room_id: u32, entity: EntityRef) -> Result<()>;
}
```

## PostgreSQL Implementation

```rust
pub struct PostgresCharacterRepository {
    pool: PgPool,
}

#[async_trait]
impl CharacterRepository for PostgresCharacterRepository {
    async fn find_by_id(&self, id: u64) -> Result<Option<Character>> {
        let row = sqlx::query_as!(
            CharacterRow,
            "SELECT * FROM characters WHERE id = $1",
            id as i64
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.into_domain()))
    }

    async fn save(&self, character: &Character) -> Result<Character> {
        let row = sqlx::query_as!(
            CharacterRow,
            r#"
            INSERT INTO characters (id, name, class, level, experience, hp, max_hp, mp, max_mp, stats, position, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                class = EXCLUDED.class,
                level = EXCLUDED.level,
                experience = EXCLUDED.experience,
                hp = EXCLUDED.hp,
                max_hp = EXCLUDED.max_hp,
                mp = EXCLUDED.mp,
                max_mp = EXCLUDED.max_mp,
                stats = EXCLUDED.stats,
                position = EXCLUDED.position
            RETURNING *
            "#,
            character.id as i64,
            character.name,
            character.class as CharacterClass,
            character.level as i32,
            character.experience as i64,
            character.hp as i32,
            character.max_hp as i32,
            character.mp as i32,
            character.max_mp as i32,
            serde_json::to_value(&character.stats)?,
            serde_json::to_value(&character.position)?,
            character.created_at,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(row.into_domain())
    }
}
```

## Database Schema

```sql
CREATE TABLE accounts (
    id BIGSERIAL PRIMARY KEY,
    username VARCHAR(50) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE characters (
    id BIGSERIAL PRIMARY KEY,
    account_id BIGINT REFERENCES accounts(id),
    name VARCHAR(50) UNIQUE NOT NULL,
    class VARCHAR(20) NOT NULL,
    level INTEGER DEFAULT 1,
    experience BIGINT DEFAULT 0,
    hp INTEGER NOT NULL,
    max_hp INTEGER NOT NULL,
    mp INTEGER DEFAULT 0,
    max_mp INTEGER DEFAULT 0,
    stats JSONB NOT NULL,
    position JSONB NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE inventory_items (
    id BIGSERIAL PRIMARY KEY,
    character_id BIGINT REFERENCES characters(id),
    item_id INTEGER NOT NULL,
    quantity INTEGER DEFAULT 1,
    UNIQUE(character_id, item_id)
);

CREATE TABLE auction_listings (
    id BIGSERIAL PRIMARY KEY,
    seller_id BIGINT REFERENCES characters(id),
    item_id INTEGER NOT NULL,
    quantity INTEGER NOT NULL,
    price BIGINT NOT NULL,
    status VARCHAR(20) DEFAULT 'active',
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ
);

CREATE TABLE rooms (
    id INTEGER PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    description TEXT NOT NULL,
    exits JSONB NOT NULL,
    npcs JSONB DEFAULT '[]',
    items JSONB DEFAULT '[]'
);

-- Indexes
CREATE INDEX idx_characters_account ON characters(account_id);
CREATE INDEX idx_characters_name ON characters(name);
CREATE INDEX idx_inventory_character ON inventory_items(character_id);
CREATE INDEX idx_auction_status ON auction_listings(status);
CREATE INDEX idx_auction_seller ON auction_listings(seller_id);
```

## Redis Integration

Redis is used for:

```rust
pub struct RedisCache {
    client: fred::clients::RedisClient,
}

impl RedisCache {
    // Session storage
    pub async fn store_session(&self, session_id: u64, data: &SessionData) -> Result<()>;
    pub async fn get_session(&self, session_id: u64) -> Result<Option<SessionData>>;
    pub async fn delete_session(&self, session_id: u64) -> Result<()>;

    // Caching
    pub async fn cache_character(&self, character: &Character) -> Result<()>;
    pub async fn get_cached_character(&self, id: u64) -> Result<Option<Character>>;
    pub async fn invalidate_character_cache(&self, id: u64) -> Result<()>;

    // Pub/Sub (for distributed events)
    pub async fn publish(&self, channel: &str, message: &[u8]) -> Result<()>;
    pub async fn subscribe(&self, channel: &str) -> Result<RedisSubscription>;

    // Distributed locks
    pub async fn acquire_lock(&self, key: &str, ttl: Duration) -> Result<Option<Lock>>;
    pub async fn release_lock(&self, lock: &Lock) -> Result<()>;

    // Rate limiting
    pub async fn check_rate_limit(&self, key: &str, limit: u32, window: Duration) -> Result<bool>;
}
```

## Connection Pooling

```rust
pub struct DatabasePool {
    pg_pool: PgPool,
    redis_client: RedisClient,
}

impl DatabasePool {
    pub async fn new(config: &DatabaseConfig) -> Result<Self> {
        let pg_options = PgPoolOptions::new()
            .max_connections(config.postgres.max_connections)
            .min_connections(config.postgres.min_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&config.postgres.url)
            .await?;

        let redis_config = fred::prelude::Config::from_url(&config.redis.url)?;
        let redis_client = RedisClient::new(redis_config, None, None, None);

        Ok(Self {
            pg_pool,
            redis_client,
        })
    }
}
```

## Configuration

```toml
[database]
[database.postgres]
url = "postgresql://user:pass@localhost:5432/the_protocol"
max_connections = 20
min_connections = 5

[database.redis]
url = "redis://localhost:6379"
max_connections = 10
```

## References

- [13-application.md](13-application.md) - Application layer using repositories
- [12-domain.md](12-domain.md) - Domain entities
