# 07-01 - 데이터베이스 통합 설계 (미구현)

## 개요

The Protocol은 PostgreSQL을 주 데이터베이스로 사용하여 게임 데이터를 영속화한다. sqlx를 사용하여 타입 안전한 쿼리를 보장하며, async/await 기반 비동기 DB 접근을 지원한다.

## PostgreSQL 통합 아키텍처

```
┌──────────────────────────────────────────────────────────┐
│                    The Protocol Server                   │
│                                                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐ │
│  │ Application │  │ Domain      │  │ Repository      │ │
│  │ Layer       │──│ Layer       │──│ Layer           │ │
│  └─────────────┘  └─────────────┘  └────────┬────────┘ │
│                                              │           │
│                                      ┌───────▼────────┐ │
│                                      │  sqlx Pool     │ │
│                                      │  (连接プール)   │ │
│                                      └───────┬────────┘ │
└──────────────────────────────────────────────┼──────────┘
                                               │
                                      ┌────────▼────────┐
                                      │   PostgreSQL    │
                                      │   Database      │
                                      └─────────────────┘
```

## 스키마 전체 명세

### accounts 테이블

```sql
CREATE TABLE accounts (
    id              BIGSERIAL PRIMARY KEY,
    username        VARCHAR(64) NOT NULL UNIQUE,
    email           VARCHAR(255) NOT NULL UNIQUE,
    password_hash   VARCHAR(255) NOT NULL,
    role            VARCHAR(20) NOT NULL DEFAULT 'player',
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login_at   TIMESTAMPTZ
);

CREATE INDEX idx_accounts_username ON accounts(username);
CREATE INDEX idx_accounts_email ON accounts(email);
```

### characters 테이블

```sql
CREATE TABLE characters (
    id              BIGSERIAL PRIMARY KEY,
    account_id      BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name            VARCHAR(32) NOT NULL UNIQUE,
    class           VARCHAR(20) NOT NULL,
    level           INTEGER NOT NULL DEFAULT 1,
    experience      BIGINT NOT NULL DEFAULT 0,
    hp              INTEGER NOT NULL,
    max_hp          INTEGER NOT NULL,
    mp              INTEGER NOT NULL,
    max_mp          INTEGER NOT NULL,
    strength        INTEGER NOT NULL,
    dexterity       INTEGER NOT NULL,
    intelligence    INTEGER NOT NULL,
    wisdom          INTEGER NOT NULL,
    constitution    INTEGER NOT NULL,
    room_id         INTEGER NOT NULL DEFAULT 1,
    gold            BIGINT NOT NULL DEFAULT 0,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_characters_account_id ON characters(account_id);
CREATE INDEX idx_characters_name ON characters(name);
CREATE INDEX idx_characters_level ON characters(level DESC);
```

### inventory_items 테이블

```sql
CREATE TABLE inventory_items (
    id              BIGSERIAL PRIMARY KEY,
    character_id    BIGINT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    item_id         INTEGER NOT NULL,
    item_name       VARCHAR(64) NOT NULL,
    quantity        INTEGER NOT NULL DEFAULT 1,
    item_type       VARCHAR(20) NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE(character_id, item_id)
);

CREATE INDEX idx_inventory_character_id ON inventory_items(character_id);
CREATE INDEX idx_inventory_item_id ON inventory_items(item_id);
```

### auction_listings 테이블

```sql
CREATE TABLE auction_listings (
    id              BIGSERIAL PRIMARY KEY,
    seller_id       BIGINT NOT NULL REFERENCES characters(id),
    item_id         INTEGER NOT NULL,
    item_name       VARCHAR(64) NOT NULL,
    quantity        INTEGER NOT NULL DEFAULT 1,
    price           BIGINT NOT NULL,
    status          VARCHAR(20) NOT NULL DEFAULT 'active',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at      TIMESTAMPTZ NOT NULL,
    buyer_id        BIGINT REFERENCES characters(id),
    sold_at         TIMESTAMPTZ
);

CREATE INDEX idx_auction_seller ON auction_listings(seller_id);
CREATE INDEX idx_auction_status ON auction_listings(status);
CREATE INDEX idx_auction_item ON auction_listings(item_id);
CREATE INDEX idx_auction_price ON auction_listings(price);
```

### rooms 테이블

```sql
CREATE TABLE rooms (
    id              INTEGER PRIMARY KEY,
    name            VARCHAR(128) NOT NULL,
    description     TEXT NOT NULL,
    exits           JSONB NOT NULL DEFAULT '{}',
    npc_ids         INTEGER[] NOT NULL DEFAULT '{}',
    item_ids        INTEGER[] NOT NULL DEFAULT '{}',
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### npcs 테이블

```sql
CREATE TABLE npcs (
    id              BIGSERIAL PRIMARY KEY,
    name            VARCHAR(64) NOT NULL,
    description     TEXT NOT NULL,
    room_id         INTEGER NOT NULL REFERENCES rooms(id),
    hp              INTEGER NOT NULL,
    max_hp          INTEGER NOT NULL,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_npcs_room ON npcs(room_id);
```

### game_logs 테이블 (감사 로그)

```sql
CREATE TABLE game_logs (
    id              BIGSERIAL PRIMARY KEY,
    player_id       BIGINT,
    action          VARCHAR(64) NOT NULL,
    details         JSONB,
    ip_address      INET,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_game_logs_player ON game_logs(player_id);
CREATE INDEX idx_game_logs_action ON game_logs(action);
CREATE INDEX idx_game_logs_created ON game_logs(created_at DESC);
```

## sqlx를 이용한 타입 안전 쿼리

### Repository Trait

```rust
use async_trait::async_trait;

#[async_trait]
pub trait CharacterRepository: Send + Sync {
    async fn find_by_id(&self, id: i64) -> Result<Option<Character>, DbError>;
    async fn find_by_name(&self, name: &str) -> Result<Option<Character>, DbError>;
    async fn find_by_account(&self, account_id: i64) -> Result<Vec<Character>, DbError>;
    async fn create(&self, character: &NewCharacter) -> Result<Character, DbError>;
    async fn update(&self, id: i64, update: &CharacterUpdate) -> Result<Character, DbError>;
    async fn delete(&self, id: i64) -> Result<(), DbError>;
    async fn update_position(&self, id: i64, room_id: i32) -> Result<(), DbError>;
    async fn update_stats(&self, id: i64, hp: i32, mp: i32) -> Result<(), DbError>;
    async fn add_experience(&self, id: i64, xp: i64) -> Result<(), DbError>;
}

#[async_trait]
pub trait AccountRepository: Send + Sync {
    async fn find_by_id(&self, id: i64) -> Result<Option<Account>, DbError>;
    async fn find_by_username(&self, username: &str) -> Result<Option<Account>, DbError>;
    async fn find_by_email(&self, email: &str) -> Result<Option<Account>, DbError>;
    async fn create(&self, account: &NewAccount) -> Result<Account, DbError>;
    async fn update_last_login(&self, id: i64) -> Result<(), DbError>;
}

#[async_trait]
pub trait InventoryRepository: Send + Sync {
    async fn find_by_character(&self, character_id: i64) -> Result<Vec<InventoryItem>, DbError>;
    async fn add_item(&self, character_id: i64, item: &NewItem) -> Result<InventoryItem, DbError>;
    async fn remove_item(&self, character_id: i64, item_id: i32, quantity: i32) -> Result<(), DbError>;
    async fn update_quantity(&self, id: i64, quantity: i32) -> Result<(), DbError>;
}

#[async_trait]
pub trait AuctionRepository: Send + Sync {
    async fn create_listing(&self, listing: &NewListing) -> Result<Listing, DbError>;
    async fn find_by_id(&self, id: i64) -> Result<Option<Listing>, DbError>;
    async fn search(&self, query: &AuctionSearch) -> Result<Vec<Listing>, DbError>;
    async fn complete_sale(&self, id: i64, buyer_id: i64) -> Result<(), DbError>;
    async fn cancel_listing(&self, id: i64) -> Result<(), DbError>;
}
```

### sqlx 쿼리 구현

```rust
use sqlx::postgres::PgPool;

pub struct PostgresCharacterRepository {
    pool: PgPool,
}

#[async_trait]
impl CharacterRepository for PostgresCharacterRepository {
    async fn find_by_id(&self, id: i64) -> Result<Option<Character>, DbError> {
        let row = sqlx::query_as!(
            Character,
            r#"
            SELECT
                id, account_id, name, class as "class: CharacterClass",
                level, experience, hp, max_hp, mp, max_mp,
                strength, dexterity, intelligence, wisdom, constitution,
                room_id, gold, is_active, created_at, updated_at
            FROM characters
            WHERE id = $1 AND is_active = TRUE
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    async fn find_by_name(&self, name: &str) -> Result<Option<Character>, DbError> {
        let row = sqlx::query_as!(
            Character,
            r#"
            SELECT
                id, account_id, name, class as "class: CharacterClass",
                level, experience, hp, max_hp, mp, max_mp,
                strength, dexterity, intelligence, wisdom, constitution,
                room_id, gold, is_active, created_at, updated_at
            FROM characters
            WHERE name = $1 AND is_active = TRUE
            "#,
            name
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    async fn find_by_account(&self, account_id: i64) -> Result<Vec<Character>, DbError> {
        let rows = sqlx::query_as!(
            Character,
            r#"
            SELECT
                id, account_id, name, class as "class: CharacterClass",
                level, experience, hp, max_hp, mp, max_mp,
                strength, dexterity, intelligence, wisdom, constitution,
                room_id, gold, is_active, created_at, updated_at
            FROM characters
            WHERE account_id = $1 AND is_active = TRUE
            ORDER BY created_at DESC
            "#,
            account_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    async fn create(&self, character: &NewCharacter) -> Result<Character, DbError> {
        let row = sqlx::query_as!(
            Character,
            r#"
            INSERT INTO characters (account_id, name, class, hp, max_hp, mp, max_mp,
                strength, dexterity, intelligence, wisdom, constitution, room_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            RETURNING
                id, account_id, name, class as "class: CharacterClass",
                level, experience, hp, max_hp, mp, max_mp,
                strength, dexterity, intelligence, wisdom, constitution,
                room_id, gold, is_active, created_at, updated_at
            "#,
            character.account_id,
            character.name,
            character.class as CharacterClass,
            character.hp,
            character.max_hp,
            character.mp,
            character.max_mp,
            character.stats.strength as i32,
            character.stats.dexterity as i32,
            character.stats.intelligence as i32,
            character.stats.wisdom as i32,
            character.stats.constitution as i32,
            character.room_id as i32,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(row)
    }

    async fn update(&self, id: i64, update: &CharacterUpdate) -> Result<Character, DbError> {
        let row = sqlx::query_as!(
            Character,
            r#"
            UPDATE characters
            SET
                level = COALESCE($2, level),
                experience = COALESCE($3, experience),
                hp = COALESCE($4, hp),
                max_hp = COALESCE($5, max_hp),
                mp = COALESCE($6, mp),
                max_mp = COALESCE($7, max_mp),
                room_id = COALESCE($8, room_id),
                gold = COALESCE($9, gold),
                updated_at = NOW()
            WHERE id = $1
            RETURNING
                id, account_id, name, class as "class: CharacterClass",
                level, experience, hp, max_hp, mp, max_mp,
                strength, dexterity, intelligence, wisdom, constitution,
                room_id, gold, is_active, created_at, updated_at
            "#,
            id,
            update.level,
            update.experience,
            update.hp,
            update.max_hp,
            update.mp,
            update.max_mp,
            update.room_id,
            update.gold,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(row)
    }

    async fn delete(&self, id: i64) -> Result<(), DbError> {
        sqlx::query!("UPDATE characters SET is_active = FALSE WHERE id = $1", id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
```

## 연결 풀 설정

```rust
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
}

impl DatabaseConfig {
    pub fn from_env() -> Result<Self, DbError> {
        Ok(Self {
            host: std::env::var("DB_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: std::env::var("DB_PORT")
                .unwrap_or_else(|_| "5432".to_string())
                .parse()
                .unwrap_or(5432),
            database: std::env::var("DB_NAME").unwrap_or_else(|_| "the_protocol".to_string()),
            username: std::env::var("DB_USER").unwrap_or_else(|_| "postgres".to_string()),
            password: std::env::var("DB_PASSWORD").unwrap_or_default(),
            max_connections: std::env::var("DB_MAX_CONN")
                .unwrap_or_else(|_| "20".to_string())
                .parse()
                .unwrap_or(20),
            min_connections: std::env::var("DB_MIN_CONN")
                .unwrap_or_else(|_| "5".to_string())
                .parse()
                .unwrap_or(5),
            connect_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(300),
        })
    }

    pub fn connection_string(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.username, self.password, self.host, self.port, self.database
        )
    }
}

pub async fn create_pool(config: &DatabaseConfig) -> Result<PgPool, DbError> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .connect_timeout(config.connect_timeout)
        .idle_timeout(config.idle_timeout)
        .connect(&config.connection_string())
        .await?;

    tracing::info!(
        max_conn = config.max_connections,
        min_conn = config.min_connections,
        "PostgreSQL connection pool created"
    );

    Ok(pool)
}
```

## 트랜잭션 관리

```rust
use sqlx::Transaction;
use sqlx::Postgres;

pub struct TransactionManager {
    pool: PgPool,
}

impl TransactionManager {
    pub async fn begin(&self) -> Result<Transaction<'_, Postgres>, DbError> {
        Ok(self.pool.begin().await?)
    }

    /// 캐릭터 간 아이템 이전 (트랜젝션 예시)
    pub async fn transfer_item(
        &self,
        from_id: i64,
        to_id: i64,
        item_id: i32,
        quantity: i32,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;

        // 1. 발송자에서 아이템 제거
        sqlx::query!(
            "UPDATE inventory_items SET quantity = quantity - $3
             WHERE character_id = $1 AND item_id = $2 AND quantity >= $3",
            from_id,
            item_id,
            quantity,
        )
        .execute(&mut *tx)
        .await?;

        // 2. 수신자에게 아이템 추가 (있으면 업데이트, 없으면 생성)
        sqlx::query!(
            "INSERT INTO inventory_items (character_id, item_id, item_name, quantity, item_type)
             SELECT $1, item_id, item_name, $4, item_type
             FROM inventory_items WHERE character_id = $2 AND item_id = $3
             ON CONFLICT (character_id, item_id) DO UPDATE SET quantity = inventory_items.quantity + $4",
            to_id,
            from_id,
            item_id,
            quantity,
        )
        .execute(&mut *tx)
        .await?;

        // 3. 발송자에서 수량 0인 아이템 정리
        sqlx::query!(
            "DELETE FROM inventory_items WHERE character_id = $1 AND item_id = $2 AND quantity <= 0",
            from_id,
            item_id,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// 경매 구매 (트랜젝션 예시)
    pub async fn purchase_auction(
        &self,
        listing_id: i64,
        buyer_id: i64,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;

        // 1. 리스팅 조회 및 잠금
        let listing = sqlx::query!(
            "SELECT id, seller_id, price, status
             FROM auction_listings WHERE id = $1 FOR UPDATE",
            listing_id,
        )
        .fetch_one(&mut *tx)
        .await?;

        if listing.status != "active" {
            tx.rollback().await?;
            return Err(DbError::BusinessError("Listing not available".to_string()));
        }

        // 2. 구매자 골드 차감
        sqlx::query!(
            "UPDATE characters SET gold = gold - $2 WHERE id = $1 AND gold >= $2",
            buyer_id,
            listing.price,
        )
        .execute(&mut *tx)
        .await?;

        // 3. 판매자 골드 추가
        sqlx::query!(
            "UPDATE characters SET gold = gold + $2 WHERE id = $1",
            listing.seller_id,
            listing.price,
        )
        .execute(&mut *tx)
        .await?;

        // 4. 아이템 이전
        sqlx::query!(
            "UPDATE inventory_items SET character_id = $2
             WHERE character_id = $1 AND item_id = (SELECT item_id FROM auction_listings WHERE id = $3)",
            listing.seller_id,
            buyer_id,
            listing_id,
        )
        .execute(&mut *tx)
        .await?;

        // 5. 리스팅 상태 업데이트
        sqlx::query!(
            "UPDATE auction_listings SET status = 'sold', buyer_id = $2, sold_at = NOW()
             WHERE id = $1",
            listing_id,
            buyer_id,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}
```

## 마이그레이션 전략 (sqlx-migrate)

```
migrations/
├── 20260828000001_create_accounts.sql
├── 20260828000002_create_characters.sql
├── 20260828000003_create_inventory.sql
├── 20260828000004_create_auction.sql
├── 20260828000005_create_rooms.sql
├── 20260828000006_create_npcs.sql
└── 20260828000007_create_game_logs.sql
```

```rust
use sqlx::migrate::Migrator;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub async fn run_migrations(pool: &PgPool) -> Result<(), DbError> {
    MIGRATOR.run(pool).await?;
    tracing::info!("Database migrations completed");
    Ok(())
}
```

## 시드 데이터

```sql
-- 기본 방 데이터
INSERT INTO rooms (id, name, description, exits, npc_ids, item_ids) VALUES
(1, 'Town Square', 'A bustling town square with a fountain.', '{"North": 2, "East": 3, "South": 4}', '{1}', '{}'),
(2, 'Forest Path', 'A winding path through a dense forest.', '{"South": 1, "North": 5}', '{2}', '{}'),
(3, 'Blacksmith Shop', 'The rhythmic clang of hammer on anvil.', '{"West": 1}', '{3}', '{1, 2}'),
(4, 'Market', 'A lively market with colorful stalls.', '{"North": 1}', '{}', '{3, 4}'),
(5, 'Goblin Cave', 'A dark, damp cave.', '{"South": 2}', '{4}', '{5}');

-- 기본 NPC 데이터
INSERT INTO npcs (id, name, description, room_id, hp, max_hp) VALUES
(1, 'Town Guard', 'A stern-looking guard.', 1, 100, 100),
(2, 'Forest Wolf', 'A gray wolf with yellow eyes.', 2, 50, 50),
(3, 'Blacksmith Garen', 'A burly man with soot-stained arms.', 3, 120, 120),
(4, 'Goblin', 'A small, green creature.', 5, 30, 30);
```

## 에러 타입

```rust
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("Record not found")]
    NotFound,

    #[error("Business rule violation: {0}")]
    BusinessError(String),

    #[error("Connection error: {0}")]
    Connection(String),
}
```
