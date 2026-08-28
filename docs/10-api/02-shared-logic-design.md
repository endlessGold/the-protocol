# 10-02 - Runtime과 Web API 간 공유 로직 설계

## 개요

The Protocol은 TCP 기반 Runtime과 HTTP 기반 Web API가 동일한 게임 규칙을 공유하는 구조를 가진다. 이를 통해 "동일한 소스 오브 진실(Single Source of Truth)"을 유지하고, 이중 구현으로 인한 버그를 방지한다.

## 아키텍처 비교

```
┌──────────────────────────────────────────────────────────┐
│                    TCP Runtime                           │
│                                                          │
│  ┌──────────┐  ┌──────────────┐  ┌───────────────────┐ │
│  │  TCP     │  │   Protocol   │  │  Application      │ │
│  │  Server  │──│   Codec      │──│  Layer            │ │
│  └──────────┘  └──────────────┘  │  (GameWorld)      │ │
│                                   └────────┬──────────┘ │
│                                            │             │
│                                  ┌─────────▼──────────┐ │
│                                  │    Domain Layer     │ │
│                                  │  (Character, World) │ │
│                                  └─────────┬──────────┘ │
│                                            │             │
│                                  ┌─────────▼──────────┐ │
│                                  │  Repository Layer  │ │
│                                  │  (DB, Cache)       │ │
│                                  └────────────────────┘ │
└──────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────┐
│                    HTTP Web API                          │
│                                                          │
│  ┌──────────┐  ┌──────────────┐  ┌───────────────────┐ │
│  │  HTTP    │  │    Axum      │  │  Application      │ │
│  │  Server  │──│   Router     │──│  Layer            │ │
│  └──────────┘  └──────────────┘  │  (GameWorld)      │ │ ← 동일
│                                   └────────┬──────────┘ │
│                                            │             │
│                                  ┌─────────▼──────────┐ │
│                                  │    Domain Layer     │ │ ← 동일
│                                  │  (Character, World) │ │
│                                  └─────────┬──────────┘ │
│                                            │             │
│                                  ┌─────────▼──────────┐ │
│                                  │  Repository Layer  │ │ ← 동일
│                                  │  (DB, Cache)       │ │
│                                  └────────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

## Application Layer 공유

`GameWorld` 구조체와 서비스 로직을 완전히 공유한다.

```rust
// application/src/service.rs - 현재 구현 (공유됨)
pub struct GameWorld {
    characters: HashMap<u64, Character>,
    world: World,
    combats: HashMap<u64, Combat>,
    next_character_id: u64,
    next_combat_id: u64,
}

impl GameWorld {
    // TCP Runtime과 HTTP API에서 동일하게 사용
    pub fn create_character(&mut self, name: String, class: &str) -> Result<Character, ApplicationError> {
        // 동일한 비즈니스 로직
    }

    pub fn look_room(&self, room_id: u32) -> Option<RoomInfo> {
        // 동일한 조회 로직
    }

    pub fn move_character(&mut self, character_id: u64, direction: Direction) -> Result<MoveResult, ApplicationError> {
        // 동일한 이동 로직
    }

    pub fn start_combat(&mut self, attacker_id: u64, target_name: &str) -> Result<CombatInfo, ApplicationError> {
        // 동일한 전투 로직
    }
}
```

### TCP Runtime에서의 사용

```rust
// core/runtime/src/main.rs
struct LookHandler {
    game_world: Arc<RwLock<GameWorld>>,
}

#[async_trait]
impl CommandHandler for LookHandler {
    async fn handle(&self, _command: Command, _session_id: u64) -> Result<CommandResponse, RoutingError> {
        let world = self.game_world.read().await;
        let room_info = world.look_room(1)  // ← 동일한 메서드 호출
            .ok_or_else(|| RoutingError::HandlerError("Room not found".to_string()))?;
        // ...
    }
}
```

### HTTP API에서의 사용

```rust
// api/src/handlers/characters.rs
pub async fn get_character(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    AuthHeader(claims): AuthHeader,
) -> Json<ApiResponse<Character>> {
    let world = state.game_world.read().await;
    let character = world.get_character(id)  // ← 동일한 메서드 호출
        .ok_or_else(|| ApiError::NotFound)?;

    Json(ApiResponse {
        success: true,
        data: Some(character.clone()),
        error: None,
        pagination: None,
    })
}
```

## Domain Layer 공유

모든 도메인 모델이 동일하다.

```rust
// domain/src/character.rs - 공유
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

// domain/src/combat.rs - 공유
pub struct Combat {
    pub id: u64,
    pub attacker_id: u64,
    pub target_id: u64,
    pub state: CombatState,
    pub turn: u32,
    pub log: Vec<CombatAction>,
}

// domain/src/event.rs - 공유
pub enum DomainEvent {
    CharacterCreated { character_id: u64, name: String },
    LevelUp { character_id: u64, new_level: u32 },
    CombatStarted { combat_id: u64, attacker_id: u64, target_id: u64 },
    AttackExecuted { combat_id: u64, attacker_id: u64, target_id: u64, damage: u32 },
    CombatEnded { combat_id: u64, winner_id: u64, loser_id: u64 },
    // ...
}
```

## Repository Layer 공유

```rust
// repository/src/lib.rs - 공유 인터페이스
#[async_trait]
pub trait CharacterRepository: Send + Sync {
    async fn find_by_id(&self, id: i64) -> Result<Option<Character>, DbError>;
    async fn find_by_name(&self, name: &str) -> Result<Option<Character>, DbError>;
    async fn create(&self, character: &NewCharacter) -> Result<Character, DbError>;
    async fn update(&self, id: i64, update: &CharacterUpdate) -> Result<Character, DbError>;
    // ...
}

// TCP Runtime과 HTTP API 모두 동일한 구현체 사용
pub struct PostgresCharacterRepository {
    pool: PgPool,
}
```

## 차이점: Transport (TCP vs HTTPS)

| 항목 | TCP Runtime | HTTP Web API |
|------|-------------|-------------|
| **프로토콜** | The Protocol (커스텀 바이너리) | HTTP/HTTPS (REST JSON) |
| **인코딩** | MessagePack + Length Prefix | JSON |
| **연결** | 지속적 (TCP) | 요청-응답 (HTTP) |
| **세션** | 서버 측 세션 관리 | JWT (무상태) |
| **실시간** | 양방향 스트림 | 폴링/WebSocket |
| **대상** | 게임 클라이언트 | 브라우저, 모바일, 외부 시스템 |

### Transport 전환 예시

```rust
// 동일한 도메인 로직, 다른 Transport

// TCP: 바이너리 프로토콜
fn handle_move_command_tcp(command: Command) -> CommandResponse {
    let move_cmd: MoveCommand = rmp_serde::from_slice(&command.payload).unwrap();
    let mut world = game_world.write().await;
    let result = world.move_character(1, move_cmd.direction).unwrap();
    // CommandResponse로 반환
}

// HTTP: REST JSON
async fn handle_move_http(
    Json(body): Json<MoveRequest>,
) -> Json<ApiResponse<MoveResponse>> {
    let mut world = game_world.write().await;
    let result = world.move_character(1, body.direction).unwrap();
    // Json ApiResponse로 반환
}
```

## 구현 전략

### 워크스페이스 구조

```
the-protocol/
├── Cargo.toml              # 워크스페이스
├── domain/                 # 도메인 레이어 (공유)
│   ├── Cargo.toml
│   └── src/
│       ├── character.rs
│       ├── combat.rs
│       ├── inventory.rs
│       ├── world.rs
│       └── event.rs
├── application/            # 어플리케이션 레이어 (공유)
│   ├── Cargo.toml
│   └── src/
│       └── service.rs
├── repository/             # 리포지토리 레이어 (공유)
│   ├── Cargo.toml
│   └── src/
│       ├── character.rs
│       ├── account.rs
│       └── inventory.rs
├── core/                   # TCP Runtime 전용
│   ├── runtime/
│   ├── network/
│   ├── protocol/
│   ├── session/
│   ├── routing/
│   ├── plugin/
│   ├── security/
│   ├── scheduler/
│   └── observability/
├── api/                    # HTTP API 전용
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── router.rs
│       ├── handlers/
│       │   ├── auth.rs
│       │   ├── characters.rs
│       │   ├── inventory.rs
│       │   ├── auction.rs
│       │   └── ranking.rs
│       └── middleware/
│           ├── auth.rs
│           └── rate_limit.rs
├── plugins/                # WASM 플러그인
└── clients/                # 클라이언트
```

### 의존성 그래프

```
domain ← application ← repository
                         ↑
                ┌────────┴────────┐
                │                  │
            core/runtime      api/
            (TCP 전용)        (HTTP 전용)
```

### 코드 공유 방법

```toml
# workspace Cargo.toml
[workspace]
members = [
    "domain",
    "application",
    "repository",
    "core/runtime",
    "core/network",
    "core/protocol",
    "api",
    # ...
]

[workspace.dependencies]
# 공유 의존성
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
sqlx = { version = "0.7", features = ["runtime-tokio", "postgres"] }
redis = { version = "0.23", features = ["tokio-comp"] }
```

```toml
# domain/Cargo.toml
[package]
name = "protocol-domain"
version = "0.1.0"

[dependencies]
serde.workspace = true
```

```toml
# application/Cargo.toml
[package]
name = "protocol-application"
version = "0.1.0"

[dependencies]
protocol-domain = { path = "../domain" }
thiserror.workspace = true
```

```toml
# api/Cargo.toml
[package]
name = "protocol-api"
version = "0.1.0"

[dependencies]
protocol-domain = { path = "../domain" }
protocol-application = { path = "../application" }
protocol-repository = { path = "../repository" }
axum = "0.7"
tokio.workspace = true
serde.workspace = true
serde_json.workspace = true
jsonwebtoken = "9"
```

## 장점

1. **버그 방지**: 동일한 비즈니스 로직이 여러 곳에 구현되지 않음
2. **테스트 효율성**: 도메인/어플리케이션 테스트가 한 번에 커버
3. **유지보수 용이성**: 로직 변경 시 한 곳만 수정
4. **일관성**: TCP와 HTTP API가 동일한 규칙 적용
5. **점진적 마이그레이션**: 기존 TCP 기반에서 HTTP API로 점진적 확장 가능
