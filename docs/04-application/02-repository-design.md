# Repository Pattern 설계 (미구현)

> **상태: ❌ 미구현** — 현재 모든 상태는 인메모리 `HashMap`으로 관리됨.

---

## 1. Repository 추상화 개요

### 1.1 왜 Repository Pattern인가?

현재 `GameWorld`는 `HashMap<u64, Character>` 등 인메모리 컬렉션에 직접 접근한다. 이 구조의 문제점:

| 문제 | 설명 |
|------|------|
| 영속성 부재 | 서버 재시작 시 모든 데이터 소멸 |
| 테스트 어려움 | 실제 상태 변경 없이 단위 테스트 불가 |
| 확장성 제한 | 수평 확장 시 상태 공유 불가 |
| 비즈니스 로직 혼재 | 데이터 접근 로직이 서비스에 섞임 |

Repository Pattern을 도입하면:
- **도메인과 인프라 분리**: 서비스는 추상 인터페이스만 의존
- **저장소 교체 용이**: 인메모리 → PostgreSQL → Redis 등 전환 비용 최소화
- **테스트 용이**: Mock Repository로 격리 테스트 가능

### 1.2 전체 아키텍처

```
Application Service (GameWorld)
        │
        ▼
Repository Trait ( 추상 인터페이스 )
        │
   ┌────┴────┐
   │         │
InMemory   PostgreSQL (+ Redis Cache)
Repository  Repository
```

---

## 2. Repository 인터페이스 전체 명세

### 2.1 CharacterRepository

```rust
#[async_trait]
pub trait CharacterRepository: Send + Sync {
    /// ID로 캐릭터 조회
    async fn find_by_id(&self, id: u64) -> Result<Option<Character>, RepositoryError>;

    /// 이름으로 캐릭터 조회
    async fn find_by_name(&self, name: &str) -> Result<Option<Character>, RepositoryError>;

    /// 계정 ID로 캐릭터 목록 조회 (다중 캐릭터 지원)
    async fn find_by_account(&self, account_id: u64) -> Result<Vec<Character>, RepositoryError>;

    /// 캐릭터 저장 (생성 또는 갱신)
    async fn save(&self, character: &Character) -> Result<(), RepositoryError>;

    /// 캐릭터 삭제
    async fn delete(&self, id: u64) -> Result<(), RepositoryError>;

    /// 이름 유일성 체크
    async fn exists_by_name(&self, name: &str) -> Result<bool, RepositoryError>;

    /// 특정 방에 있는 캐릭터 목록 조회
    async fn find_by_room(&self, room_id: u32) -> Result<Vec<Character>, RepositoryError>;

    /// 캐릭터 배치 저장 (동기화 최적화)
    async fn save_batch(&self, characters: &[Character]) -> Result<(), RepositoryError>;
}
```

**메서드별 용도:**

| 메서드 | 복잡도 | 설명 |
|--------|--------|------|
| `find_by_id` | O(1) | PK 조회, 캐시 히트율 높음 |
| `find_by_name` | O(1) | Unique 인덱스 활용 |
| `find_by_account` | O(1) | FK 인덱스 활용 |
| `save` | O(1) | UPSERT (INSERT ON CONFLICT UPDATE) |
| `delete` | O(1) | Soft delete 권장 |
| `exists_by_name` | O(1) | 이름 생성 시 빠른 체크 |
| `find_by_room` | O(n) | 방별 인덱스 필요 |
| `save_batch` | O(n) | 전투 결과 등 배치 처리 |

### 2.2 InventoryRepository

```rust
#[async_trait]
pub trait InventoryRepository: Send + Sync {
    /// 플레이어 인벤토리 조회
    async fn find_by_player(&self, player_id: u64) -> Result<Option<Inventory>, RepositoryError>;

    /// 인벤토리 저장
    async fn save(&self, player_id: u64, inventory: &Inventory) -> Result<(), RepositoryError>;

    /// 아이템 추가 (원자적)
    async fn add_item(
        &self,
        player_id: u64,
        item_id: u32,
        name: &str,
        quantity: u32,
    ) -> Result<(), RepositoryError>;

    /// 아이템 제거 (원자적)
    async fn remove_item(
        &self,
        player_id: u64,
        item_id: u32,
        quantity: u32,
    ) -> Result<(), RepositoryError>;

    /// 아이템 보유 확인
    async fn has_item(
        &self,
        player_id: u64,
        item_id: u32,
        quantity: u32,
    ) -> Result<bool, RepositoryError>;
}
```

**설계 고려사항:**
- `add_item` / `remove_item`은 원자적 트랜잭션 필요 (동시 구매/판매 시 race condition 방지)
- `inventory` 테이블: `player_id` (FK), `item_id`, `name`, `quantity`, `capacity`, `gold`

### 2.3 AuctionRepository

```rust
#[async_trait]
pub trait AuctionRepository: Send + Sync {
    /// 경매 아이템 조회
    async fn find_by_id(&self, auction_id: u64) -> Result<Option<AuctionItem>, RepositoryError>;

    /// 활성 경매 목록 조회 (만료 안된 것만)
    async fn find_active(
        &self,
        item_type: Option<ItemType>,
        min_price: Option<u64>,
        max_price: Option<u64>,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<AuctionItem>, RepositoryError>;

    /// 경매 등록
    async fn save(&self, item: &AuctionItem) -> Result<(), RepositoryError>;

    /// 경매 삭제 (만료/취소)
    async fn delete(&self, auction_id: u64) -> Result<(), RepositoryError>;

    /// 입찰 기록 저장
    async fn save_bid(&self, bid: &AuctionBid) -> Result<(), RepositoryError>;

    /// 입찰 기록 조회
    async fn find_bids(&self, auction_id: u64) -> Result<Vec<AuctionBid>, RepositoryError>;

    /// 만료된 경매 목록 조회 (스케줄러용)
    async fn find_expired(&self, before: chrono::DateTime<chrono::Utc>)
        -> Result<Vec<AuctionItem>, RepositoryError>;
}
```

### 2.4 WorldRepository

```rust
#[async_trait]
pub trait WorldRepository: Send + Sync {
    /// 방 조회
    async fn get_room(&self, room_id: u32) -> Result<Option<Room>, RepositoryError>;

    /// NPC 조회
    async fn get_npc(&self, npc_id: u64) -> Result<Option<Npc>, RepositoryError>;

    /// 방에 엔티티 추가 (NPC/아이템)
    async fn add_entity(
        &self,
        room_id: u32,
        entity_type: EntityType,
        entity_id: u64,
    ) -> Result<(), RepositoryError>;

    /// 방에서 엔티티 제거
    async fn remove_entity(
        &self,
        room_id: u32,
        entity_type: EntityType,
        entity_id: u64,
    ) -> Result<(), RepositoryError>;

    /// 모든 방 조회 (월드 초기화)
    async fn get_all_rooms(&self) -> Result<Vec<Room>, RepositoryError>;

    /// 모든 NPC 조회
    async fn get_all_npcs(&self) -> Result<Vec<Npc>, RepositoryError>;

    /// 방의 NPC 목록 조회
    async fn get_room_npcs(&self, room_id: u32) -> Result<Vec<Npc>, RepositoryError>;

    /// 방의 아이템 목록 조회
    async fn get_room_items(&self, room_id: u32) -> Result<Vec<Item>, RepositoryError>;
}

pub enum EntityType {
    Npc,
    Item,
}
```

### 2.5 CombatRepository

```rust
#[async_trait]
pub trait CombatRepository: Send + Sync {
    /// 전투 조회
    async fn find_by_id(&self, combat_id: u64) -> Result<Option<Combat>, RepositoryError>;

    /// 특정 캐릭터의 활성 전투 조회
    async fn find_active_by_character(
        &self,
        character_id: u64,
    ) -> Result<Option<Combat>, RepositoryError>;

    /// 전투 저장 (생성 또는 갱신)
    async fn save(&self, combat: &Combat) -> Result<(), RepositoryError>;

    /// 전투 삭제
    async fn delete(&self, combat_id: u64) -> Result<(), RepositoryError>;

    /// 만료된 전투 정리 (비활동 전투 제거)
    async fn cleanup_stale(&self, max_age: std::time::Duration) -> Result<u64, RepositoryError>;
}
```

---

## 3. RepositoryError 정의

```rust
#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Not found: {entity} with id {id}")]
    NotFound { entity: String, id: String },

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),

    #[error("Timeout")]
    Timeout,
}
```

---

## 4. PostgreSQL 구현 (sqlx)

### 4.1 연결 풀 설정

```rust
use sqlx::postgres::{PgPoolOptions, PgPool};

pub struct PostgresCharacterRepository {
    pool: PgPool,
}

impl PostgresCharacterRepository {
    pub async fn new(database_url: &str) -> Result<Self, RepositoryError> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .min_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(5))
            .idle_timeout(std::time::Duration::from_secs(300))
            .max_lifetime(std::time::Duration::from_secs(1800))
            .connect(database_url)
            .await
            .map_err(|e| RepositoryError::ConnectionFailed(e.to_string()))?;

        Ok(Self { pool })
    }
}
```

**연결 풀 튜닝 가이드:**

| 환경 | max_connections | min_connections | timeout |
|------|-----------------|-----------------|---------|
| 개발 | 5 | 1 | 5s |
| 운영 소규모 | 20 | 5 | 5s |
| 운영 대규모 | 50 | 10 | 3s |

### 4.2 스키마 설계

```sql
-- 캐릭터 테이블
CREATE TABLE characters (
    id          BIGSERIAL PRIMARY KEY,
    account_id  BIGINT NOT NULL DEFAULT 0,
    name        VARCHAR(32) NOT NULL UNIQUE,
    class       VARCHAR(16) NOT NULL,
    level       INTEGER NOT NULL DEFAULT 1,
    experience  BIGINT NOT NULL DEFAULT 0,
    hp          INTEGER NOT NULL,
    max_hp      INTEGER NOT NULL,
    mp          INTEGER NOT NULL,
    max_mp      INTEGER NOT NULL,
    stats       JSONB NOT NULL,
    room_id     INTEGER NOT NULL DEFAULT 1,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_characters_account ON characters(account_id);
CREATE INDEX idx_characters_room ON characters(room_id);
CREATE INDEX idx_characters_name_lower ON characters(LOWER(name));

-- 인벤토리 테이블
CREATE TABLE inventories (
    id          SERIAL PRIMARY KEY,
    player_id   BIGINT NOT NULL REFERENCES characters(id),
    item_id     INTEGER NOT NULL,
    name        VARCHAR(64) NOT NULL,
    quantity    INTEGER NOT NULL DEFAULT 1,
    UNIQUE(player_id, item_id)
);

CREATE INDEX idx_inventories_player ON inventories(player_id);

-- 골드는 별도 컬럼으로 관리
CREATE TABLE player_gold (
    player_id   BIGINT PRIMARY KEY REFERENCES characters(id),
    gold        BIGINT NOT NULL DEFAULT 0
);

-- 방 테이블
CREATE TABLE rooms (
    id          INTEGER PRIMARY KEY,
    name        VARCHAR(128) NOT NULL,
    description TEXT NOT NULL,
    exits       JSONB NOT NULL DEFAULT '{}',
    npc_ids     BIGINT[] NOT NULL DEFAULT '{}',
    item_ids    INTEGER[] NOT NULL DEFAULT '{}'
);

-- NPC 테이블
CREATE TABLE npcs (
    id          BIGSERIAL PRIMARY KEY,
    name        VARCHAR(64) NOT NULL,
    description TEXT NOT NULL,
    room_id     INTEGER NOT NULL REFERENCES rooms(id),
    hp          INTEGER NOT NULL,
    max_hp      INTEGER NOT NULL
);

CREATE INDEX idx_npcs_room ON npcs(room_id);

-- 전투 테이블
CREATE TABLE combats (
    id          BIGSERIAL PRIMARY KEY,
    attacker_id BIGINT NOT NULL REFERENCES characters(id),
    target_id   BIGINT NOT NULL,
    state       VARCHAR(16) NOT NULL,
    turn        INTEGER NOT NULL DEFAULT 1,
    log         JSONB NOT NULL DEFAULT '[]',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_combats_attacker ON combats(attacker_id);
CREATE INDEX idx_combats_state ON combats(state);

-- 경매 테이블
CREATE TABLE auctions (
    id          BIGSERIAL PRIMARY KEY,
    seller_id   BIGINT NOT NULL REFERENCES characters(id),
    item_id     INTEGER NOT NULL,
    item_name   VARCHAR(64) NOT NULL,
    quantity    INTEGER NOT NULL DEFAULT 1,
    starting_price BIGINT NOT NULL,
    current_price   BIGINT NOT NULL,
    buyout_price    BIGINT,
    highest_bidder  BIGINT REFERENCES characters(id),
    state       VARCHAR(16) NOT NULL DEFAULT 'active',
    expires_at  TIMESTAMPTZ NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_auctions_state ON auctions(state);
CREATE INDEX idx_auctions_item ON auctions(item_id);
CREATE INDEX idx_auctions_expires ON auctions(expires_at);
```

### 4.3 쿼리 최적화

**N+1 조회 방지:**

```rust
// 잘못된 예 (N+1)
let characters = room.npc_ids.iter()
    .map(|id| npc_repo.find_by_id(*id))  // NPC 수만큼 DB 조회
    .collect::<Vec<_>>();

// 올바른 예 (Batch 조회)
let npcs = npc_repo.find_by_ids(&room.npc_ids).await?;
```

**Bulk UPSERT:**

```rust
// 배치 저장 최적화
async fn save_batch(&self, characters: &[Character]) -> Result<(), RepositoryError> {
    sqlx::query(
        r#"
        INSERT INTO characters (id, name, class, level, experience, hp, max_hp, mp, max_mp, stats, room_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (id) DO UPDATE SET
            level = EXCLUDED.level,
            experience = EXCLUDED.experience,
            hp = EXCLUDED.hp,
            max_hp = EXCLUDED.max_hp,
            mp = EXCLUDED.mp,
            max_mp = EXCLUDED.max_mp,
            stats = EXCLUDED.stats,
            room_id = EXCLUDED.room_id,
            updated_at = NOW()
        "#
    )
    // ... 바인딩
    .execute(&self.pool)
    .await?;
    Ok(())
}
```

### 4.4 트랜잭션 관리

```rust
// 전투 결과 저장 시 트랜잭션
async fn save_combat_result(
    &self,
    combat: &Combat,
    attacker: &Character,
    target_hp: u32,
) -> Result<(), RepositoryError> {
    let mut tx = self.pool.begin().await
        .map_err(|e| RepositoryError::TransactionFailed(e.to_string()))?;

    // 전투 상태 갱신
    sqlx::query("UPDATE combats SET state = $1, turn = $2 WHERE id = $3")
        .bind(&combat.state)
        .bind(combat.turn)
        .bind(combat.id)
        .execute(&mut *tx)
        .await?;

    // 공격자 상태 갱신 (경험치 등)
    sqlx::query("UPDATE characters SET experience = $1, level = $2 WHERE id = $3")
        .bind(attacker.experience)
        .bind(attacker.level)
        .bind(attacker.id)
        .execute(&mut *tx)
        .await?;

    // NPC HP 갱신
    sqlx::query("UPDATE npcs SET hp = $1 WHERE id = $2")
        .bind(target_hp)
        .bind(combat.target_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await
        .map_err(|e| RepositoryError::TransactionFailed(e.to_string()))?;

    Ok(())
}
```

---

## 5. Redis 캐시 레이어

### 5.1 캐시 전략 비교

| 전략 | 설명 | 장점 | 단점 | 적합场景 |
|------|------|------|------|----------|
| **Write-through** | 쓰기 시 DB+캐시 동시 갱신 | 일관성 보장 | 쓰기 지연 | 방 정보, NPC 상태 |
| **Write-back** | 쓰기 시 캐시만 갱신, 비동기 DB 반영 | 빠른 쓰기 | 데이터 유실 리스크 | 전투 로그, 이벤트 |
| **Cache-aside** | 읽기 시 캐시 miss → DB 조회 → 캐시 저장 | 유연성 | 첫 조회 지연 | 캐릭터 정보 |

**The Protocol 권장 조합:**
- 캐릭터 정보: **Write-through** (읽기 빈도 높음, 갱신 빈도 낮음)
- 방/NPC 상태: **Write-through** (다중 플레이어 공유)
- 전투 정보: **Cache-aside** (임시 데이터, 만료 기반)
- 경매: **Cache-aside** (검색 결과 캐싱)

### 5.2 캐시 키 설계

```
# 캐릭터
character:{id}              → Character JSON (TTL: 300s)
character:name:{name}       → character_id (TTL: 300s)
character:room:{room_id}    → Set of character_ids (TTL: 60s)

# 방
room:{id}                   → Room JSON (TTL: 600s)
room:{id}:players           → Set of character_ids (TTL: 60s)
room:{id}:npcs              → Set of npc_ids (TTL: 120s)

# 인벤토리
inventory:{player_id}       → Inventory JSON (TTL: 300s)

# 전투
combat:{id}                 → Combat JSON (TTL: 1800s)
combat:active:{character_id} → combat_id (TTL: 1800s)

# 경매
auction:active              → Sorted Set (score: expires_at, TTL: 3600s)
auction:item:{item_id}      → AuctionItem JSON (TTL: 3600s)

# 세션
session:{id}                → Session data (TTL: 1800s)
```

### 5.3 캐시 구현

```rust
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

pub struct RedisCache {
    conn: ConnectionManager,
    default_ttl: u64,
}

impl RedisCache {
    pub async fn new(redis_url: &str) -> Result<Self, RepositoryError> {
        let client = redis::Client::open(redis_url)
            .map_err(|e| RepositoryError::ConnectionFailed(e.to_string()))?;
        let conn = client.get_tokio_connection_manager().await
            .map_err(|e| RepositoryError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            conn,
            default_ttl: 300,
        })
    }

    /// 캐릭터 조회 (캐시 우선)
    pub async fn get_character(&self, id: u64) -> Result<Option<Character>, RepositoryError> {
        let key = format!("character:{}", id);
        let data: Option<String> = self.conn.get(&key).await
            .map_err(|e| RepositoryError::Database(e.to_string()))?;

        match data {
            Some(json) => {
                let character: Character = serde_json::from_str(&json)
                    .map_err(|e| RepositoryError::Serialization(e.to_string()))?;
                Ok(Some(character))
            }
            None => Ok(None),
        }
    }

    /// 캐릭터 저장 (Write-through)
    pub async fn set_character(&self, character: &Character) -> Result<(), RepositoryError> {
        let key = format!("character:{}", character.id);
        let json = serde_json::to_string(character)
            .map_err(|e| RepositoryError::Serialization(e.to_string()))?;

        self.conn.setex(&key, self.default_ttl, &json).await
            .map_err(|e| RepositoryError::Database(e.to_string()))?;

        // 이름 → ID 매핑도 저장
        let name_key = format!("character:name:{}", character.name);
        self.conn.setex(&name_key, self.default_ttl, character.id).await
            .map_err(|e| RepositoryError::Database(e.to_string()))?;

        Ok(())
    }

    /// 캐시 무효화
    pub async fn invalidate_character(&self, id: u64) -> Result<(), RepositoryError> {
        let key = format!("character:{}", id);
        self.conn.del(&key).await
            .map_err(|e| RepositoryError::Database(e.to_string()))?;
        Ok(())
    }
}
```

### 5.4 캐시-DB 동기화

```rust
// 캐시된 Repository 어댑터
pub struct CachedCharacterRepository {
    db: PostgresCharacterRepository,
    cache: RedisCache,
}

#[async_trait]
impl CharacterRepository for CachedCharacterRepository {
    async fn find_by_id(&self, id: u64) -> Result<Option<Character>, RepositoryError> {
        // 1. 캐시 확인
        if let Some(character) = self.cache.get_character(id).await? {
            return Ok(Some(character));
        }

        // 2. DB 조회
        if let Some(character) = self.db.find_by_id(id).await? {
            // 3. 캐시 저장
            self.cache.set_character(&character).await?;
            return Ok(Some(character));
        }

        Ok(None)
    }

    async fn save(&self, character: &Character) -> Result<(), RepositoryError> {
        // 1. DB 저장
        self.db.save(character).await?;

        // 2. 캐시 갱신 (Write-through)
        self.cache.set_character(character).await?;

        Ok(())
    }
}
```

### 5.5 Redis Pub/Sub 캐시 무효화

다중 인스턴스 환경에서 캐시 무효화 동기화:

```rust
// 캐시 무효화 이벤트
#[derive(Serialize, Deserialize)]
struct CacheInvalidation {
    entity_type: String,  // "character", "room", etc.
    entity_id: u64,
    timestamp: u64,
}

// 발행
pub async fn publish_invalidation(
    &self,
    entity_type: &str,
    entity_id: u64,
) -> Result<(), RepositoryError> {
    let event = CacheInvalidation {
        entity_type: entity_type.to_string(),
        entity_id,
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
    };
    let json = serde_json::to_string(&event).unwrap();
    self.pubsub_conn.publish("cache:invalidate", &json).await
        .map_err(|e| RepositoryError::Database(e.to_string()))?;
    Ok(())
}

// 구독
pub async fn subscribe_invalidation(&self) {
    let mut pubsub = self.pubsub_conn.as_pubsub();
    pubsub.subscribe("cache:invalidate").await.unwrap();

    loop {
        let msg: String = pubsub.get_message().await.unwrap().get_payload().unwrap();
        let event: CacheInvalidation = serde_json::from_str(&msg).unwrap();
        self.cache.invalidate(&event.entity_type, event.entity_id).await;
    }
}
```

---

## 6. Repository 테스트 전략

### 6.1 테스트 더블

```rust
// Mock Repository (테스트용)
pub struct MockCharacterRepository {
    characters: Arc<RwLock<HashMap<u64, Character>>>,
}

#[async_trait]
impl CharacterRepository for MockCharacterRepository {
    async fn find_by_id(&self, id: u64) -> Result<Option<Character>, RepositoryError> {
        let chars = self.characters.read().await;
        Ok(chars.get(&id).cloned())
    }

    async fn save(&self, character: &Character) -> Result<(), RepositoryError> {
        let mut chars = self.characters.write().await;
        chars.insert(character.id, character.clone());
        Ok(())
    }

    // ... 나머지 메서드 구현
}
```

### 6.2 통합 테스트 (PostgreSQL)

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;

    async fn setup_test_db() -> PostgresCharacterRepository {
        let url = std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://localhost/the_protocol_test".to_string());

        let repo = PostgresCharacterRepository::new(&url).await.unwrap();

        // 테스트 전 데이터 정리
        sqlx::query("TRUNCATE characters CASCADE")
            .execute(&repo.pool)
            .await
            .unwrap();

        repo
    }

    #[tokio::test]
    async fn test_create_and_find_character() {
        let repo = setup_test_db().await;

        let character = Character::new("TestHero".to_string(), CharacterClass::Warrior);
        repo.save(&character).await.unwrap();

        let found = repo.find_by_id(character.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "TestHero");
    }

    #[tokio::test]
    async fn test_name_uniqueness() {
        let repo = setup_test_db().await;

        let c1 = Character::new("Unique".to_string(), CharacterClass::Mage);
        repo.save(&c1).await.unwrap();

        let exists = repo.exists_by_name("Unique").await.unwrap();
        assert!(exists);
    }
}
```

### 6.3 성능 테스트

```rust
#[tokio::test]
async fn test_bulk_save_performance() {
    let repo = setup_test_db().await;

    let characters: Vec<Character> = (0..10000)
        .map(|i| Character::new(format!("Char{}", i), CharacterClass::Warrior))
        .collect();

    let start = std::time::Instant::now();
    repo.save_batch(&characters).await.unwrap();
    let elapsed = start.elapsed();

    println!("Saved 10,000 characters in {:?}", elapsed);
    assert!(elapsed < std::time::Duration::from_secs(5));
}
```

---

## 7. 마이그레이션 전략

### 7.1 인메모리 → PostgreSQL 전환

1단계: 인메모리 Repository 구현 (현재 상태를 trait로 래핑)
2단계: PostgreSQL Repository 구현 및 병렬 테스트
3단계: 설정 기반 저장소 전환 지원
4단계: 데이터 마이그레이션 스크립트

```rust
pub enum StorageBackend {
    InMemory,
    Postgres { url: String },
    Cached { db_url: String, redis_url: String },
}

impl ServiceContainer {
    pub fn from_config(config: &Config) -> Self {
        match config.storage {
            StorageBackend::InMemory => { /* 현재 구조 */ }
            StorageBackend::Postgres { url } => { /* PG 구조 */ }
            StorageBackend::Cached { db_url, redis_url } => { /* 캐시 구조 */ }
        }
    }
}
```
