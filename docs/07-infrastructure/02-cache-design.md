# 07-02 - Redis 캐시 설계 (미구현)

## 개요

The Protocol은 Redis를 캐시 레이어로 사용하여 자주 접근되는 데이터의 응답 시간을 개선하고, 세션 스토리지, 분산 락, 이벤트 퍼블리싱을 구현한다.

## 캐시 레이어 아키텍처

```
┌──────────────────────────────────────────────────────────┐
│                    The Protocol Server                   │
│                                                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐ │
│  │ Application │  │ Repository  │  │ Cache Layer     │ │
│  │ Layer       │──│ Layer       │──│ (Redis)         │ │
│  └─────────────┘  └──────┬──────┘  └────────┬────────┘ │
│                          │                   │           │
│                   ┌──────▼──────┐   ┌────────▼────────┐ │
│                   │ PostgreSQL  │   │     Redis       │ │
│                   │ (Primary)   │   │   (Cache)       │ │
│                   └─────────────┘   └─────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

### Cache-Aside 패턴

```
1. READ: Cache Hit → 반환
         Cache Miss → DB 조회 → Cache 저장 → 반환

2. WRITE: DB 업데이트 → Cache 무효화
          또는 DB 업데이트 → Cache 업데이트 (동시)
```

## 캐시 전략

### Session Storage: 플레이어 세션

플레이어의 실시간 세션 데이터를 Redis에 저장하여 멀티 서버 환경에서 세션 공유를 지원한다.

```
Key:    session:{session_id}
Type:   Hash
Fields:
  - player_id     : u64
  - account_id    : u64
  - character_id  : u64
  - room_id       : u32
  - state         : String (Connected, Authenticating, Authenticated, InGame)
  - transport     : String (Tcp, Udp, WebSocket)
  - connected_at  : String (ISO 8601)
  - last_activity : String (ISO 8601)
TTL:    3600초 (1시간, 비활성 시 자동 만료)
```

```rust
use redis::AsyncCommands;

pub struct SessionCache {
    client: redis::Client,
    default_ttl: u64,
}

impl SessionCache {
    pub async fn set_session(
        &self,
        session_id: u64,
        data: &SessionData,
    ) -> Result<(), CacheError> {
        let mut conn = self.client.get_async_connection().await?;

        let key = format!("session:{}", session_id);
        let _: () = redis::cmd("HSET")
            .arg(&key)
            .arg("player_id")
            .arg(data.player_id)
            .arg("account_id")
            .arg(data.account_id)
            .arg("character_id")
            .arg(data.character_id)
            .arg("room_id")
            .arg(data.room_id)
            .arg("state")
            .arg(&data.state)
            .arg("transport")
            .arg(&data.transport)
            .arg("connected_at")
            .arg(&data.connected_at)
            .arg("last_activity")
            .arg(&data.last_activity)
            .query_async(&mut conn)
            .await?;

        let _: () = conn.expire(&key, self.default_ttl as i64).await?;

        Ok(())
    }

    pub async fn get_session(
        &self,
        session_id: u64,
    ) -> Result<Option<SessionData>, CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("session:{}", session_id);

        let data: Option<HashMap<String, String>> = redis::cmd("HGETALL")
            .arg(&key)
            .query_async(&mut conn)
            .await?;

        Ok(data.map(|h| SessionData {
            player_id: h.get("player_id").and_then(|v| v.parse().ok()).unwrap_or(0),
            account_id: h.get("account_id").and_then(|v| v.parse().ok()).unwrap_or(0),
            character_id: h.get("character_id").and_then(|v| v.parse().ok()).unwrap_or(0),
            room_id: h.get("room_id").and_then(|v| v.parse().ok()).unwrap_or(1),
            state: h.get("state").cloned().unwrap_or_default(),
            transport: h.get("transport").cloned().unwrap_or_default(),
            connected_at: h.get("connected_at").cloned().unwrap_or_default(),
            last_activity: h.get("last_activity").cloned().unwrap_or_default(),
        }))
    }

    pub async fn delete_session(
        &self,
        session_id: u64,
    ) -> Result<(), CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("session:{}", session_id);
        let _: () = conn.del(&key).await?;
        Ok(())
    }

    pub async fn touch_session(
        &self,
        session_id: u64,
    ) -> Result<(), CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("session:{}", session_id);
        let _: () = conn.expire(&key, self.default_ttl as i64).await?;
        Ok(())
    }
}
```

### Character Cache: 캐릭터 데이터

자주 접근되는 캐릭터 데이터를 캐시하여 DB 조회를 줄인다.

```
Key:    character:{character_id}
Type:   Hash
Fields:
  - id             : u64
  - name           : String
  - class          : String
  - level          : u32
  - experience     : u64
  - hp             : u32
  - max_hp         : u32
  - mp             : u32
  - max_mp         : u32
  - strength       : u32
  - dexterity      : u32
  - intelligence   : u32
  - wisdom         : u32
  - constitution   : u32
  - room_id        : u32
  - gold           : u64
  - updated_at     : String
TTL:    300초 (5분)
```

```rust
pub struct CharacterCache {
    client: redis::Client,
}

impl CharacterCache {
    pub async fn set_character(
        &self,
        character: &Character,
    ) -> Result<(), CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("character:{}", character.id);

        let _: () = redis::cmd("HSET")
            .arg(&key)
            .arg("id").arg(character.id)
            .arg("name").arg(&character.name)
            .arg("class").arg(format!("{:?}", character.class))
            .arg("level").arg(character.level)
            .arg("experience").arg(character.experience)
            .arg("hp").arg(character.hp)
            .arg("max_hp").arg(character.max_hp)
            .arg("mp").arg(character.mp)
            .arg("max_mp").arg(character.max_mp)
            .arg("strength").arg(character.stats.strength)
            .arg("dexterity").arg(character.stats.dexterity)
            .arg("intelligence").arg(character.stats.intelligence)
            .arg("wisdom").arg(character.stats.wisdom)
            .arg("constitution").arg(character.stats.constitution)
            .arg("room_id").arg(character.room_id)
            .arg("gold").arg(character.gold)
            .query_async(&mut conn)
            .await?;

        let _: () = conn.expire(&key, 300).await?;
        Ok(())
    }

    pub async fn get_character(
        &self,
        character_id: u64,
    ) -> Result<Option<Character>, CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("character:{}", character_id);

        let data: Option<HashMap<String, String>> = redis::cmd("HGETALL")
            .arg(&key)
            .query_async(&mut conn)
            .await?;

        // HashMap → Character 변환
        Ok(data.map(|h| parse_character_from_hash(&h)))
    }

    pub async fn invalidate_character(
        &self,
        character_id: u64,
    ) -> Result<(), CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("character:{}", character_id);
        let _: () = conn.del(&key).await?;
        Ok(())
    }
}
```

### Room Cache: 방 정보

방 데이터는 변동이 적으므로 긴 TTL로 캐시한다.

```
Key:    room:{room_id}
Type:   Hash
Fields:
  - id             : u32
  - name           : String
  - description    : String
  - exits          : String (JSON)
  - npc_ids        : String (JSON Array)
  - item_ids       : String (JSON Array)
TTL:    3600초 (1시간)
```

```rust
pub struct RoomCache {
    client: redis::Client,
}

impl RoomCache {
    pub async fn set_room(&self, room: &Room) -> Result<(), CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("room:{}", room.id);

        let exits_json = serde_json::to_string(&room.exits)?;
        let npc_ids_json = serde_json::to_string(&room.npc_ids)?;
        let item_ids_json = serde_json::to_string(&room.item_ids)?;

        let _: () = redis::cmd("HSET")
            .arg(&key)
            .arg("id").arg(room.id)
            .arg("name").arg(&room.name)
            .arg("description").arg(&room.description)
            .arg("exits").arg(&exits_json)
            .arg("npc_ids").arg(&npc_ids_json)
            .arg("item_ids").arg(&item_ids_json)
            .query_async(&mut conn)
            .await?;

        let _: () = conn.expire(&key, 3600).await?;
        Ok(())
    }

    pub async fn get_room(&self, room_id: u32) -> Result<Option<Room>, CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("room:{}", room_id);

        let data: Option<HashMap<String, String>> = redis::cmd("HGETALL")
            .arg(&key)
            .query_async(&mut conn)
            .await?;

        Ok(data.map(|h| parse_room_from_hash(&h)))
    }
}
```

### Leaderboard: 랭킹

Redis Sorted Set을 사용하여 실시간 랭킹을 관리한다.

```
Key:    leaderboard:level
Type:   Sorted Set
Score:  character_level (동률 시 experience 사용)
Member: character_id

Key:    leaderboard:combat
Type:   Sorted Set
Score:  total_kills
Member: character_id
```

```rust
pub struct LeaderboardCache {
    client: redis::Client,
}

impl LeaderboardCache {
    /// 레벨 랭킹 업데이트
    pub async fn update_level_ranking(
        &self,
        character_id: u64,
        level: u32,
        experience: u64,
    ) -> Result<(), CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = "leaderboard:level";

        // 레벨 * 1_000_000 + 경험치로 단일 score 생성
        let score = (level as f64) * 1_000_000.0 + (experience as f64);

        let _: () = redis::cmd("ZADD")
            .arg(key)
            .arg(score)
            .arg(character_id)
            .query_async(&mut conn)
            .await?;

        Ok(())
    }

    /// 상위 N명 조회
    pub async fn get_top_players(
        &self,
        count: i64,
    ) -> Result<Vec<(u64, f64)>, CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = "leaderboard:level";

        let results: Vec<(u64, f64)> = redis::cmd("ZREVRANGE")
            .arg(key)
            .arg(0)
            .arg(count - 1)
            .arg("WITHSCORES")
            .query_async(&mut conn)
            .await?;

        Ok(results)
    }

    /// 특정 플레이어 랭킹 조회
    pub async fn get_player_rank(
        &self,
        character_id: u64,
    ) -> Result<Option<(u64, f64)>, CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = "leaderboard:level";

        let rank: Option<i64> = redis::cmd("ZREVRANK")
            .arg(key)
            .arg(character_id)
            .query_async(&mut conn)
            .await?;

        let score: Option<f64> = redis::cmd("ZSCORE")
            .arg(key)
            .arg(character_id)
            .query_async(&mut conn)
            .await?;

        Ok(rank.zip(score).map(|(r, s)| (r as u64 + 1, s)))
    }
}
```

## 캐시 키 설계

```
┌────────────────┬────────────────────────┬───────────┐
│      패턴       │        예시             │    TTL    │
├────────────────┼────────────────────────┼───────────┤
│ session:{id}   │ session:12345          │ 3600초    │
│ character:{id} │ character:100          │ 300초     │
│ room:{id}      │ room:1                 │ 3600초    │
│ leaderboard:*  │ leaderboard:level      │ 갱신 시   │
│ auction:search │ auction:search:weapon  │ 60초      │
│ player:{id}:*  │ player:100:inventory   │ 120초     │
│ lock:*         │ lock:auction:12345     │ 10초      │
│ rate:{ip}      │ rate:192.168.1.1       │ 60초      │
└────────────────┴────────────────────────┴───────────┘
```

## TTL 전략

| 데이터 타입 | TTL | 이유 |
|-----------|-----|------|
| 세션 | 1시간 | 비활성 세션 자동 만료 |
| 캐릭터 | 5분 | 빈번한 업데이트 반영 |
| 방 | 1시간 | 변동 적음 |
| 랭킹 | 수동 갱신 | 이벤트 기반 |
| 경매 검색 | 1분 | 빈번한 변경 |
| Rate Limit | 1분 | 윈도우 기반 |

## 캐시 Invalidation

```rust
pub struct CacheInvalidator {
    redis: redis::Client,
}

impl CacheInvalidator {
    /// 캐릭터 업데이트 시 캐시 무효화
    pub async fn on_character_update(
        &self,
        character_id: u64,
    ) -> Result<(), CacheError> {
        let mut conn = self.redis.get_async_connection().await?;

        // 캐릭터 캐시 무효화
        let char_key = format!("character:{}", character_id);
        let _: () = conn.del(&char_key).await?;

        // 관련 세션 캐시도 업데이트
        let session_keys: Vec<String> = redis::cmd("KEYS")
            .arg("session:*")
            .query_async(&mut conn)
            .await?;

        for key in session_keys {
            let field_char_id: Option<String> = redis::cmd("HGET")
                .arg(&key)
                .arg("character_id")
                .query_async(&mut conn)
                .await?;

            if let Some(cid) = field_char_id {
                if cid == character_id.to_string() {
                    // 세션의 last_activity 갱신
                    let _: () = conn.expire(&key, 3600).await?;
                }
            }
        }

        Ok(())
    }

    /// 방 업데이트 시 캐시 무효화
    pub async fn on_room_update(
        &self,
        room_id: u32,
    ) -> Result<(), CacheError> {
        let mut conn = self.redis.get_async_connection().await?;
        let key = format!("room:{}", room_id);
        let _: () = conn.del(&key).await?;
        Ok(())
    }

    /// 경매 변경 시 검색 캐시 무효화
    pub async fn on_auction_change(
        &self,
    ) -> Result<(), CacheError> {
        let mut conn = self.redis.get_async_connection().await?;
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg("auction:search:*")
            .query_async(&mut conn)
            .await?;

        for key in keys {
            let _: () = conn.del(&key).await?;
        }
        Ok(())
    }
}
```

## Redis Pub/Sub (이벤트)

게임 이벤트를 Redis Pub/Sub을 통해 다른 서버 인스턴스에 브로드캐스트한다.

```
Channel: game:events
Payload: {
    "event_type": "player_entered_room",
    "player_id": 100,
    "room_id": 1,
    "timestamp": "2026-08-28T10:00:00Z"
}
```

```rust
pub struct EventBus {
    publisher: redis::Client,
    subscriber: redis::Client,
}

impl EventBus {
    pub async fn publish(
        &self,
        event: &GameEvent,
    ) -> Result<(), CacheError> {
        let mut conn = self.publisher.get_async_connection().await?;
        let payload = serde_json::to_string(event)?;

        let _: () = redis::cmd("PUBLISH")
            .arg("game:events")
            .arg(&payload)
            .query_async(&mut conn)
            .await?;

        Ok(())
    }

    pub async fn subscribe(
        &self,
    ) -> Result<redis::PubSub, CacheError> {
        let mut pubsub = self.subscriber.get_async_connection().await?;
        pubsub.subscribe("game:events").await?;
        Ok(pubsub)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum GameEvent {
    PlayerEnteredRoom { player_id: u64, room_id: u32 },
    PlayerLeftRoom { player_id: u64, room_id: u32 },
    CharacterLevelUp { character_id: u64, new_level: u32 },
    CombatStarted { combat_id: u64, attacker_id: u64, target_id: u64 },
    CombatEnded { combat_id: u64, winner_id: u64 },
    AuctionCreated { listing_id: u64 },
    AuctionSold { listing_id: u64, buyer_id: u64 },
}
```

## 분산 락

멀티 서버 환경에서 동시성 제어를 위한 Redis 분산 락.

```rust
pub struct DistributedLock {
    client: redis::Client,
}

impl DistributedLock {
    /// 락 획득
    pub async fn acquire(
        &self,
        resource: &str,
        ttl_ms: u64,
    ) -> Result<Option<String>, CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let lock_value = uuid::Uuid::new_v4().to_string();
        let key = format!("lock:{}", resource);

        let acquired: bool = redis::cmd("SET")
            .arg(&key)
            .arg(&lock_value)
            .arg("NX")
            .arg("PX")
            .arg(ttl_ms)
            .query_async(&mut conn)
            .await?;

        if acquired {
            Ok(Some(lock_value))
        } else {
            Ok(None)
        }
    }

    /// 락 해제 (값 비교 후 삭제)
    pub async fn release(
        &self,
        resource: &str,
        lock_value: &str,
    ) -> Result<bool, CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("lock:{}", resource);

        // Lua 스크립트로 원자적 해제
        let script = r#"
            if redis.call("GET", KEYS[1]) == ARGV[1] then
                return redis.call("DEL", KEYS[1])
            else
                return 0
            end
        "#;

        let result: i32 = redis::cmd("EVAL")
            .arg(script)
            .arg(1)
            .arg(&key)
            .arg(lock_value)
            .query_async(&mut conn)
            .await?;

        Ok(result == 1)
    }

    /// 락 갱신 (TTL 연장)
    pub async fn renew(
        &self,
        resource: &str,
        lock_value: &str,
        ttl_ms: u64,
    ) -> Result<bool, CacheError> {
        let mut conn = self.client.get_async_connection().await?;
        let key = format!("lock:{}", resource);

        let script = r#"
            if redis.call("GET", KEYS[1]) == ARGV[1] then
                return redis.call("PEXPIRE", KEYS[1], ARGV[2])
            else
                return 0
            end
        "#;

        let result: i32 = redis::cmd("EVAL")
            .arg(script)
            .arg(1)
            .arg(&key)
            .arg(lock_value)
            .arg(ttl_ms)
            .query_async(&mut conn)
            .await?;

        Ok(result == 1)
    }
}
```

**분산 락 사용 예시:**
```rust
// 경매 구매 시 동시성 제어
let lock = DistributedLock::new(redis_client.clone());
if let Some(token) = lock.acquire("auction:12345", 5000).await? {
    // 경매 처리
    let result = process_auction_purchase(listing_id, buyer_id).await;

    // 락 해제
    lock.release("auction:12345", &token).await?;
    result
} else {
    Err(CacheError::LockBusy)
}
```
