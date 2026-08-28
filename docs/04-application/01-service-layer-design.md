# Application Service Layer 상세 설계

## 1. 서비스 레이어 아키텍처

### 1.1 레이어 위치

```
┌─────────────────────────────────────────────┐
│           Network Layer (TCP/UDP)            │
│  core/network, core/protocol, core/session   │
├─────────────────────────────────────────────┤
│         Routing Layer (CommandRouter)        │
│           core/routing                      │
├─────────────────────────────────────────────┤
│      ▼▼▼  APPLICATION LAYER  ▼▼▼           │
│           application/src/service.rs        │
│         GameWorld 서비스 (핵심)              │
├─────────────────────────────────────────────┤
│           Domain Layer                       │
│  domain/ (character, combat, inventory,      │
│           world, event)                      │
└─────────────────────────────────────────────┘
```

Application Layer는 Network Layer와 Domain Layer 사이에 위치하며, 클라이언트 요청을 도메인 로직으로 변환하는 어댑터 역할을 수행한다. 현재 `GameWorld` 구조체가 모든 서비스 로직을 단일 struct에 통합하는 모놀리식 구조로 구현되어 있다.

### 1.2 현재 구현 구조

현재 Application Layer는 다음 파일들로 구성된다:

| 파일 | 역할 |
|------|------|
| `application/src/lib.rs` | 모듈 재export |
| `application/src/service.rs` | GameWorld 서비스 전체 구현 |

핵심 구조체인 `GameWorld`는 다음 상태를 관리한다:

```rust
pub struct GameWorld {
    characters: HashMap<u64, Character>,  // 모든 캐릭터 (인메모리)
    world: World,                         // 월드 상태 (방, NPC)
    combats: HashMap<u64, Combat>,        // 활성 전투 세션
    next_character_id: u64,               // ID 시퀀스
    next_combat_id: u64,                  // 전투 ID 시퀀스
}
```

**현재 한계점:**
- 모든 상태가 인메모리 (프로세스 종료 시 소멸)
- RwLock `Arc<RwLock<GameWorld>>`으로 전역 공유 — 동시성 병목 가능
- 서비스 인터페이스 추상화 없음 (trait 미사용)
- 의존성 주입 미적용

---

## 2. GameWorld 서비스 상세 분석

### 2.1 `create_character(name, class)`

**시그니처:**
```rust
pub fn create_character(&mut self, name: String, class: &str)
    -> Result<Character, ApplicationError>
```

**처리 흐름:**

1. **클래스 검증**: `CharacterClass::from_str(class)` 호출. 지원 클래스: `Warrior`, `Mage`, `Rogue`, `Cleric`. 미지원 클래스는 `ApplicationError::InvalidCharacterName` 반환.

2. **이름 유일성 체크**: `self.characters.values().any(|c| c.name == name)` — 전체 캐릭터 순회로 O(n) 복잡도. 동일 이름 존재 시 `ApplicationError::CharacterNameTaken` 반환.

3. **캐릭터 생성**: `Character::new(name, class)` 호출. 도메인 계층에서 기본 스탯 계산:
   - `max_hp = 50 + (constitution * 2)`
   - `max_mp = 20 + wisdom`
   - 각 클래스별 기본 스탯 상이 (Warrior: STR 15, Mage: INT 15 등)

4. **ID 할당**: `next_character_id`에서 증분 할당 (시퀀스 방식, DB 전환 시 SERIAL/auto_increment 대체 필요)

5. **시작 방 배정**: `character.room_id = 1` (Town Square 하드코딩)

6. **검증 누락 사항:**
   - 이름 길이 제한 없음
   - 이름 특수문자 검증 없음
   - 중복 캐릭터 생성 방지 로직 없음 (같은 계정에서 다수 생성 가능)

**반환값:** `Character` 구조체 (id 할당 후). 호출측에서 `add_character()`로 별도 저장 필요.

### 2.2 `look_room(room_id)`

**시그니처:**
```rust
pub fn look_room(&self, room_id: u32) -> Option<RoomInfo>
```

**처리 흐름:**

1. **방 조회**: `self.world.get_room(room_id)` — `World.rooms: HashMap<u32, Room>`에서 직접 조회

2. **플레이어 정보 수집**: `self.characters.values()`를 순회하여 `c.room_id == room_id` 필터링. O(n) 복잡도.

3. **NPC 정보 수집**: `room.npc_ids`를 순회하며 `self.world.get_npc(*id)` 호출. Room이 참조하는 NPC ID 목록을 기반으로 N+1 조회 발생 가능.

4. **출구 정보**: `room.exits.keys()`를 direction 문자열 목록으로 변환

5. **RoomInfo DTO 반환:**
```rust
pub struct RoomInfo {
    pub name: String,
    pub description: String,
    pub exits: Vec<String>,
    pub players: Vec<PlayerSummary>,
    pub npcs: Vec<NpcSummary>,
}
```

**성능 고려사항:**
- 플레이어 수가 많은 방은 `characters` 전체 순회 오버헤드
- NPC 조회 시 `room.npc_ids` → `world.npcs` 이중 조회
- 아이템 정보(`room.item_ids`)는 RoomInfo에 포함되지 않음 (미구현)

### 2.3 `move_character(character_id, direction)`

**시그니처:**
```rust
pub fn move_character(
    &mut self,
    character_id: u64,
    direction: Direction,
) -> Result<MoveResult, ApplicationError>
```

**처리 흐름:**

1. **캐릭터 존재 확인**: `self.characters.get(&character_id)` — 없으면 `CharacterNotFound`

2. **현재 방 조회**: `self.world.get_room(current_room_id)` — 없으면 `NoExit`

3. **출구 확인**: `current_room.exits.get(&direction)` — 해당 방향 출구 없으면 `NoExit`

4. **위치 갱신**: `character.room_id = new_room_id`

5. **새 방 정보 조회**: 이동 후 방 이름/설명 반환

6. **검증 누락 사항:**
   - 이동 중 전투 상태 확인 없음
   - 문/포탈 잠금 검증 없음
   - 무거운 아이템으로 인한 이동 제한 없음
   - 같은 방으로의 이동 (up/down 등) 검증 없음

**반환값:**
```rust
pub struct MoveResult {
    pub from_room_id: u32,
    pub to_room_id: u32,
    pub room_name: String,
    pub room_description: String,
}
```

### 2.4 `start_combat(attacker_id, target_name)`

**시그니처:**
```rust
pub fn start_combat(
    &mut self,
    attacker_id: u64,
    target_name: &str,
) -> Result<CombatInfo, ApplicationError>
```

**처리 흐름:**

1. **공격자 조회**: `self.characters.get(&attacker_id)` — `.clone()`으로 전체 복사 (불필요한 복사 오버헤드)

2. **자기 공격 방지**: `attacker_id == 0` 체크 — 실제 캐릭터 ID 검증이 아닌 하드코딩 값 비교로, 의도된 검증이 아님

3. **타겟 NPC 조회**: `self.world.find_npc_in_room(attacker.room_id, target_name)` — 같은 방의 NPC를 이름 부분 일치로 검색

4. **전투 생성**: `Combat::new(attacker_id, target_npc.id)` — 도메인 계층에서 Combat 구조체 생성

5. **데미지 계산**: `Combat::calculate_damage(attacker, &Character { ... })` — NPC를 Character로 변환하는 임시 구조체 생성 (구조적 비효율)
   - 공격 공식: `base_damage = strength`, `defense = constitution * 0.5`
   - 변동폭: `raw_damage * 0.2` 범위 내 랜덤
   - 최소 데미지: 1

6. **NPC HP 갱신**: `target_npc.hp = target_npc.hp.saturating_sub(damage)`

7. **전투 기록**: `self.combats.insert(combat_id, combat)`

**현재 제한사항:**
- 턴 기반 전투 미구현 (즉시 데미지 처리)
- 플레이어 간 전투 미구현
- 방어/스킬/마법 미구현
- 전투 종료 조건 (NPC 사망) 미처리 — `CombatState::Finished` 전환은 도메인에서 처리하지만 서비스에서 미반영
- 경험치 획득 미반영 (도메인 `process_attack` 호출 안함)

### 2.5 `get_inventory(character_id)`

**시그니처:**
```rust
pub fn get_inventory(&self, character_id: u64) -> Option<&Inventory>
```

**처리 흐름:**

단순 조회: `self.characters.get(&character_id).map(|c| &c.inventory)`

현재 인벤토리는 완전히 비어있음 (새 캐릭터 생성 시 `Inventory::new()`로 빈 인벤토리 할당). 아이템 추가/제거/사용 로직은 서비스 레이어에 없음.

---

## 3. 서비스 인터페이스 설계

### 3.1 현재 상태: 인터페이스 부재

현재 `GameWorld`는 trait 없이 구체 타입으로 직접 사용된다. `CommandHandler` trait을 구현한 각 Handler 구조체가 `Arc<RwLock<GameWorld>>`를 직접 주입받는다.

```rust
// 현재: 직접 의존성
struct LookHandler {
    game_world: Arc<RwLock<GameWorld>>,
}
```

### 3.2 향후 trait 기반 인터페이스 설계

```rust
#[async_trait]
pub trait GameService: Send + Sync {
    // 캐릭터 관리
    async fn create_character(&self, session_id: u64, name: String, class: &str)
        -> Result<Character, ApplicationError>;
    async fn get_character(&self, id: u64)
        -> Result<Character, ApplicationError>;

    // 월드 조회
    async fn look_room(&self, room_id: u32)
        -> Result<RoomInfo, ApplicationError>;

    // 이동
    async fn move_character(&self, character_id: u64, direction: Direction)
        -> Result<MoveResult, ApplicationError>;

    // 전투
    async fn start_combat(&self, attacker_id: u64, target_name: &str)
        -> Result<CombatInfo, ApplicationError>;

    // 인벤토리
    async fn get_inventory(&self, character_id: u64)
        -> Result<Inventory, ApplicationError>;
}

#[async_trait]
pub trait AuthService: Send + Sync {
    async fn login(&self, username: &str, password: &str)
        -> Result<Session, ApplicationError>;
    async fn logout(&self, session_id: u64)
        -> Result<(), ApplicationError>;
}

#[async_trait]
pub trait ShopService: Send + Sync {
    async fn buy_item(&self, player_id: u64, npc_id: u64, item_id: u32)
        -> Result<(), ApplicationError>;
    async fn sell_item(&self, player_id: u64, item_id: u32, quantity: u32)
        -> Result<u64, ApplicationError>;
}
```

---

## 4. 의존성 주입 패턴

### 4.1 현재 패턴: 직접 주입

```rust
// runtime/src/main.rs
let game_world = Arc::new(RwLock::new(GameWorld::new()));

let gw = game_world.clone();
command_router.register("look", Arc::new(LookHandler { game_world: gw }));
```

모든 Handler가 동일한 `Arc<RwLock<GameWorld>>`에 의존. 테스트 시 Mock 불가.

### 4.2 향후 DI 컨테이너 패턴

```rust
pub struct ServiceContainer {
    pub game_service: Arc<dyn GameService>,
    pub auth_service: Arc<dyn AuthService>,
    pub event_bus: Arc<dyn EventBus>,
    pub scheduler: Arc<dyn SchedulerService>,
}

impl ServiceContainer {
    pub fn new() -> Self {
        // 인메모리 구현 (개발/테스트)
        // 또는 PostgreSQL/Redis 기반 구현 (운영)
        let character_repo = Arc::new(InMemoryCharacterRepository::new());
        let world_repo = Arc::new(InMemoryWorldRepository::new());

        Self {
            game_service: Arc::new(DefaultGameService::new(
                character_repo.clone(),
                world_repo.clone(),
            )),
            auth_service: Arc::new(DefaultAuthService::new()),
            event_bus: Arc::new(InProcessEventBus::new()),
            scheduler: Arc::new(TokioScheduler::new()),
        }
    }
}
```

### 4.3 핵심 의존성 그래프

```
CommandRouter
  ├── LookHandler ──────→ GameService (trait)
  ├── MoveHandler ──────→ GameService (trait)
  ├── AttackHandler ────→ GameService (trait)
  ├── InventoryHandler ─→ GameService (trait)
  └── CreateHandler ────→ GameService (trait)
                               │
                          ┌────┴────┐
                   CharacterRepo  WorldRepo
                   (trait)        (trait)
                       │              │
                 InMemory/Postgres  InMemory/Postgres
```

---

## 5. 에러 처리 전략

### 5.1 `ApplicationError` 전체 명세

```rust
#[derive(Debug, Error)]
pub enum ApplicationError {
    // 캐릭터 관련
    CharacterNotFound(u64),        // 지정 ID의 캐릭터 미존재
    CharacterNameTaken(String),    // 이름 중복
    InvalidCharacterName(String),  // 유효하지 않은 클래스명

    // NPC 관련
    NpcNotFound(u64),              // 지정 ID의 NPC 미존재

    // 이동 관련
    NoExit,                        // 해당 방향에 출구 없음

    // 전투 관련
    CombatNotFound(u64),           // 지정 ID의 전투 미존재 (미사용)
    TargetNotInSameRoom,           // 타겟이 같은 방에 없음 (미사용)
    TargetDead,                    // 타겟이 사망 (미사용)
    SelfAttack,                    // 자기 공격 시도

    // 인벤토리 관련
    ItemNotFound(u32),             // 지정 ID의 아이템 미존재 (미사용)
}
```

### 5.2 에러 매핑 전략

```
ApplicationError ──→ RoutingError ──→ CommandResponse
                      │                  │
                      ├─ UnknownCommand  ├─ success: false
                      └─ HandlerError    └─ error: Some(message)
```

Handler 내에서 `ApplicationError`를 `RoutingError::HandlerError`로 변환하여 반환한다. CommandResponse의 `success` 필드와 `error` 필드로 클라이언트에 전달.

### 5.3 에러 처리 개선 방향

| 현재 | 개선 |
|------|------|
| `ApplicationError` → `String` 변환 | 에러 코드 체계 도입 (E001, E002 등) |
| 클라이언트에 에러 메시지 직접 노출 | 에러 메시지 다국화 지원 |
| 모든 에러가 동일 레벨 | 에러 심각도 분리 (Warning, Error, Critical) |
| 로깅 없음 | `tracing::error!` 통한 구조화된 에러 로깅 |

---

## 6. 현재 구현 vs 미구현

### 6.1 구현 완료

| 서비스 | 상태 | 설명 |
|--------|------|------|
| `GameWorld` | ✅ 구현 | 인메모리 기반 전체 서비스 |
| `create_character` | ✅ 구현 | 이름 중복 체크, 클래스 검증 |
| `look_room` | ✅ 구현 | 방/플레이어/NPC 조회 |
| `move_character` | ✅ 구현 | 방향 기반 이동 |
| `start_combat` | ✅ 구현 | 기본 데미지 계산 |
| `get_inventory` | ✅ 구현 | 단순 조회 |

### 6.2 미구현 서비스

| 서비스 | 우선순위 | 설명 |
|--------|----------|------|
| ❌ `AuthenticationService` | 높음 | 로그인/로그아웃/세션 관리. 현재 하드코딩된 character_id=1로 대체 |
| ❌ `ShopService` | 중간 | 아이템 구매/판매. 상점 NPC와의 트랜잭션 |
| ❌ `ChatService` | 중간 | 채팅/메시징. 귓속말/채널/전체 채팅 |
| ❌ `GuildService` | 낮음 | 길드 생성/관리/길드전 |
| ❌ `AuctionService` | 낮음 | 경매장 시스템. 입찰/물건 등록/만료 |

### 6.3 서비스별 설계 요건

**AuthenticationService:**
- JWT 또는 세션 기반 인증
- 패스워드 해싱 (bcrypt/argon2)
- 세션-캐릭터 매핑
- 동시 로그인 제한
- 토큰 갱신 로직

**ShopService:**
- NPC별 상점 아이템 목록
- 골드 잔액 검증
- 아이템 재고 관리
- 거래 내역 기록
- 할인/프리미엄 적용

**ChatService:**
- 채널 시스템 (전체/지역/근처/귓속말/길드)
- 메시지 필터링 (비속어/스팸)
- 메시지 큐잉 (비동기 전송)
- 채팅 기록 저장

**GuildService:**
- 길드 생성/해체
- 멤버 초대/강퇴
- 길드 랭킹
- 길드 스킬/버프

**AuctionService:**
- 물건 등록/취소
- 입찰/즉시구매
- 만료 자동 처리
- 수수료 계산

### 6.4 인프라 미구현 사항

| 항목 | 상태 | 설명 |
|------|------|------|
| Repository Pattern | ❌ | DB 연결 미적용 |
| Event Bus | ❌ | 이벤트 발행/구독 미적용 |
| 캐시 레이어 | ❌ | Redis 등 캐시 미적용 |
| 로깅 시스템 | ⚠️ 부분 | `tracing` 기반이나 상세 로깅 부족 |
| 메트릭스 | ❌ | Prometheus 등 메트릭 미연동 |

---

## 7. 레퍼런스

### 7.1 현재 관련 소스 파일

| 경로 | 라인 | 설명 |
|------|------|------|
| `application/src/service.rs` | 7-38 | `ApplicationError` 정의 |
| `application/src/service.rs` | 40-46 | `GameWorld` 구조체 |
| `application/src/service.rs` | 59-77 | `create_character()` |
| `application/src/service.rs` | 95-128 | `look_room()` |
| `application/src/service.rs` | 130-159 | `move_character()` |
| `application/src/service.rs` | 161-228 | `start_combat()` |
| `application/src/service.rs` | 230-232 | `get_inventory()` |
| `domain/src/character.rs` | 83-139 | `Character` 도메인 로직 |
| `domain/src/combat.rs` | 36-102 | `Combat` 도메인 로직 |
| `core/routing/src/lib.rs` | 19-21 | `CommandHandler` trait |
| `core/routing/src/lib.rs` | 39-49 | `CommandRouter::route()` |
| `core/runtime/src/main.rs` | 55-91 | 서버 초기화 및 Handler 등록 |
| `core/runtime/src/main.rs` | 342-569 | CommandHandler 구현체 |
