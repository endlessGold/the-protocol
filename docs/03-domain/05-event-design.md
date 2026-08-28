# 도메인 이벤트 시스템 상세 설계

> 모듈: `domain::event`
> 소스: `domain/src/event.rs`

---

## 1. 이벤트 시스템 아키텍처

### 1.1 전체 구조도

```
┌─────────────────────────────────────────────────────────┐
│                  Domain Layer                            │
│                                                          │
│  Character  ──┐                                         │
│  Combat     ──┼──▶ DomainEvent 발생                     │
│  Inventory  ──┤                                         │
│  World      ──┘                                         │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│              DomainEvent 열거형                          │
│                                                          │
│  CharacterCreated                                       │
│  LevelUp                                                │
│  CombatStarted                                          │
│  AttackExecuted                                         │
│  CombatEnded                                            │
│  PlayerEnteredRoom                                      │
│  PlayerLeftRoom                                         │
│  ItemAcquired                                           │
│  ItemRemoved                                            │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│              Application Layer (향후)                    │
│                                                          │
│  이벤트 버스 (EventBus)                                  │
│  ├─ 핸들러 등록                                          │
│  ├─ 이벤트 발행                                          │
│  ├─ 이벤트 필터링                                        │
│  └─ 이벤트 로깅                                          │
│                                                          │
│  상태 동기화                                              │
│  ├─ 캐릭터 상태 갱신                                      │
│  ├─ 월드 상태 갱신                                        │
│  └─ 클라이언트 알림                                       │
└─────────────────────────────────────────────────────────┘
```

### 1.2 이벤트 발생 흐름

```
1. 유저 액션 (커맨드 입력)
2. Application Layer: 커맨드 처리
3. Domain Layer: 비즈니스 로직 실행
4. Domain Layer: 이벤트 발생 (DomainEvent 생성)
5. Application Layer: 이벤트 수집
6. Application Layer: 이벤트 처리 (상태 갱신, 클라이언트 알림)
```

### 1.3 이벤트 특성

| 특성 | 설명 |
|------|------|
| **불변성 (Immutable)** | 이벤트는 생성 후 변경되지 않음 |
| **과거 시제** | 이미 발생한 사실을 기술 ("~했다") |
| **원자성** | 하나의 이벤트는 하나의 상태 변경을 나타냄 |
| **순서 보장** | 이벤트 발생 순서는 보장되지 않음 (event sourcing 시 필요) |
| **비동기 처리 가능** | 이벤트 버스를 통해 비동기 핸들링 가능 |

---

## 2. DomainEvent 열거형 전체 명세

### 2.1 9개 이벤트 정의

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEvent {
    // === 캐릭터 관련 (2개) ===
    CharacterCreated { character_id: u64, name: String },
    LevelUp { character_id: u64, new_level: u32 },

    // === 전투 관련 (3개) ===
    CombatStarted { combat_id: u64, attacker_id: u64, target_id: u64 },
    AttackExecuted { combat_id: u64, attacker_id: u64, target_id: u64, damage: u32 },
    CombatEnded { combat_id: u64, winner_id: u64, loser_id: u64 },

    // === 월드 관련 (2개) ===
    PlayerEnteredRoom { player_id: u64, room_id: u32 },
    PlayerLeftRoom { player_id: u64, room_id: u32 },

    // === 인벤토리 관련 (2개) ===
    ItemAcquired { player_id: u64, item_id: u32, quantity: u32 },
    ItemRemoved { player_id: u64, item_id: u32, quantity: u32 },
}
```

### 2.2 이벤트별 상세 명세

#### 2.2.1 CharacterCreated

| 항목 | 내용 |
|------|------|
| **발생 조건** | 캐릭터 생성 성공 시 |
| **필드** | `character_id: u64`, `name: String` |
| **발생 위치** | `application/src/service.rs` — `create_character()` |
| **반응** | 월드에 캐릭터 추가, 클라이언트에 캐릭터 정보 전달 |

#### 2.2.2 LevelUp

| 항목 | 내용 |
|------|------|
| **발생 조건** | `experience ≥ xp_for_next_level()` |
| **필드** | `character_id: u64`, `new_level: u32` |
| **발생 위치** | `domain/src/character.rs` — `gain_experience()` |
| **반응** | 캐릭터 스탯 업데이트, 알림 메시지 |

**발생 로직:**
```rust
while self.experience >= self.xp_for_next_level() {
    self.experience -= self.xp_for_next_level();
    self.level += 1;
    self.max_hp += 10;
    self.hp = self.max_hp;
    events.push(DomainEvent::LevelUp {
        character_id: self.id,
        new_level: self.level,
    });
}
```

#### 2.2.3 CombatStarted

| 항목 | 내용 |
|------|------|
| **발생 조건** | 전투 시작 시 |
| **필드** | `combat_id: u64`, `attacker_id: u64`, `target_id: u64` |
| **발생 위치** | `application/src/service.rs` — `start_combat()` |
| **반응** | 전투 상태 초기화, 참가자에게 전투 시작 알림 |

#### 2.2.4 AttackExecuted

| 항목 | 내용 |
|------|------|
| **발생 조건** | 공격이 데미지를 입혔을 때 |
| **필드** | `combat_id: u64`, `attacker_id: u64`, `target_id: u64`, `damage: u32` |
| **발생 위치** | `domain/src/combat.rs` — `process_attack()` |
| **반응** | 데미지 적용, 전투 로그 기록, HP 업데이트 |

**발생 로직:**
```rust
let damage = Self::calculate_damage(attacker, target);
target.take_damage(damage);

events.push(DomainEvent::AttackExecuted {
    combat_id: self.id,
    attacker_id: self.attacker_id,
    target_id: self.target_id,
    damage,
});
```

#### 2.2.5 CombatEnded

| 항목 | 내용 |
|------|------|
| **발생 조건** | 한쪽이 사망했을 때 |
| **필드** | `combat_id: u64`, `winner_id: u64`, `loser_id: u64` |
| **발생 위치** | `domain/src/combat.rs` — `process_attack()` |
| **반응** | 전투 종료, 보상 처리, 경험치 보상 |

#### 2.2.6 PlayerEnteredRoom

| 항목 | 내용 |
|------|------|
| **발생 조건** | 플레이어가 새 방에 진입했을 때 |
| **필드** | `player_id: u64`, `room_id: u32` |
| **발생 위치** | `application/src/service.rs` — `move_character()` |
| **반응** | 해당 방의 다른 플레이어에게 입장 알림 |

#### 2.2.7 PlayerLeftRoom

| 항목 | 내용 |
|------|------|
| **발생 조건** | 플레이어가 방을 떠났을 때 |
| **필드** | `player_id: u64`, `room_id: u32` |
| **발생 위치** | `application/src/service.rs` — `move_character()` |
| **반응** | 해당 방의 다른 플레이어에게 퇴장 알림 |

#### 2.2.8 ItemAcquired

| 항목 | 내용 |
|------|------|
| **발생 조건** | 아이템을 획득했을 때 |
| **필드** | `player_id: u64`, `item_id: u32`, `quantity: u32` |
| **발생 위치** | `application/src/service.rs` — 아이템 획득 로직 |
| **반응** | 인벤토리 업데이트, 획득 메시지 |

#### 2.2.9 ItemRemoved

| 항목 | 내용 |
|------|------|
| **발생 조건** | 아이템을 사용/드롭/교환했을 때 |
| **필드** | `player_id: u64`, `item_id: u32`, `quantity: u32` |
| **발생 위치** | `application/src/service.rs` — 아이템 사용 로직 |
| **반응** | 인벤토리 업데이트, 소비 메시지 |

---

## 3. 이벤트 발생 조건

### 3.1 이벤트 매핑 테이블

| 이벤트 | 발생 메서드 | 조건 | 반환 타입 |
|--------|-------------|------|-----------|
| `CharacterCreated` | `create_character()` | 이름 유니크 + 클래스 유효 | `Result<Character>` |
| `LevelUp` | `gain_experience()` | `experience ≥ xp_for_next_level()` | `Vec<DomainEvent>` |
| `CombatStarted` | `start_combat()` | 공격자/대상 유효, 같은 방 | `Result<CombatInfo>` |
| `AttackExecuted` | `process_attack()` | 전투 진행 중 | `Vec<DomainEvent>` |
| `CombatEnded` | `process_attack()` | `target.is_alive() == false` | `Vec<DomainEvent>` |
| `PlayerEnteredRoom` | `move_character()` | 출구 존재 | `Result<MoveResult>` |
| `PlayerLeftRoom` | `move_character()` | 출구 존재 | `Result<MoveResult>` |
| `ItemAcquired` | (미구현) | 아이템 획득 시 | - |
| `ItemRemoved` | (미구현) | 아이템 소비 시 | - |

### 3.2 이벤트 발생 조건 검증

```rust
// 이벤트 발생 전 검증이 필요한 경우
pub fn validate_event_trigger(event: &DomainEvent) -> bool {
    match event {
        DomainEvent::LevelUp { character_id, new_level } => {
            *new_level > 0 && *new_level <= 100
        }
        DomainEvent::AttackExecuted { damage, .. } => {
            *damage > 0
        }
        DomainEvent::ItemAcquired { quantity, .. } |
        DomainEvent::ItemRemoved { quantity, .. } => {
            *quantity > 0
        }
        _ => true,  // 기타 이벤트는 별도 검증 없음
    }
}
```

---

## 4. 이벤트 핸들링 플로우

### 4.1 현재 핸들링 방식

현재 이벤트는 **즉시 처리** 방식:

```
커맨드 입력
    │
    ▼
Domain 로직 실행
    │
    ▼
이벤트 생성 (DomainEvent)
    │
    ▼
즉시 상태 변경 (이벤트 핸들러 없음)
    │
    ▼
결과 반환
```

### 4.2 향후 핸들링 방식 (이벤트 버스)

```
커맨드 입력
    │
    ▼
Domain 로직 실행
    │
    ▼
이벤트 생성 (DomainEvent)
    │
    ▼
이벤트 버스에 발행
    │
    ├──▶ 핸들러 1: 상태 동기화
    ├──▶ 핸들러 2: 클라이언트 알림
    ├──▶ 핸들러 3: 로깅
    └──▶ 핸들러 4: 분석
    │
    ▼
비동기 처리 완료 대기
```

### 4.3 이벤트 핸들러 예시

```rust
// 이벤트 핸들러 인터페이스 (설계)
pub trait EventHandler {
    fn handle(&self, event: &DomainEvent);
}

// 상태 동기화 핸들러
pub struct StateSyncHandler;

impl EventHandler for StateSyncHandler {
    fn handle(&self, event: &DomainEvent) {
        match event {
            DomainEvent::LevelUp { character_id, new_level } => {
                // 캐릭터 상태 캐시 갱신
                println!("Character {} leveled up to {}", character_id, new_level);
            }
            DomainEvent::PlayerEnteredRoom { player_id, room_id } => {
                // 방 인원 업데이트
                println!("Player {} entered room {}", player_id, room_id);
            }
            _ => {}
        }
    }
}

// 클라이언트 알림 핸들러
pub struct NotificationHandler;

impl EventHandler for NotificationHandler {
    fn handle(&self, event: &DomainEvent) {
        match event {
            DomainEvent::AttackExecuted { attacker_id, target_id, damage, .. } => {
                // 공격자/대상에게 데미지 알림
                println!("Attack: {} → {} ({} damage)", attacker_id, target_id, damage);
            }
            DomainEvent::CombatEnded { winner_id, loser_id, .. } => {
                // 승패 알림
                println!("Combat ended: {} defeated {}", winner_id, loser_id);
            }
            _ => {}
        }
    }
}
```

---

## 5. 이벤트 버스 구현 (미구현)

### 5.1 이벤트 버스 인터페이스

```rust
pub trait EventBus {
    /// 이벤트 발행
    fn publish(&self, event: DomainEvent);

    /// 핸들러 등록
    fn subscribe(&mut self, handler: Box<dyn EventHandler>);

    /// 특정 이벤트 타입에 대한 핸들러 등록
    fn subscribe_filtered<F>(&mut self, filter: F, handler: Box<dyn EventHandler>)
    where
        F: Fn(&DomainEvent) -> bool + 'static;

    /// 핸들러 제거
    fn unsubscribe(&mut self, handler_id: usize);

    /// 이벤트 큐 처리 (비동기)
    fn process_queue(&mut self);
}
```

### 5.2 구독/발행 패턴

```rust
pub struct InMemoryEventBus {
    handlers: Vec<(usize, Box<dyn EventHandler>)>,
    event_queue: VecDeque<DomainEvent>,
    next_handler_id: usize,
    event_log: Vec<DomainEvent>,  // 이벤트 히스토리
}

impl InMemoryEventBus {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            event_queue: VecDeque::new(),
            next_handler_id: 0,
            event_log: Vec::new(),
        }
    }
}

impl EventBus for InMemoryEventBus {
    fn publish(&self, event: DomainEvent) {
        self.event_log.push(event.clone());
        for (_, handler) in &self.handlers {
            handler.handle(&event);
        }
    }

    fn subscribe(&mut self, handler: Box<dyn EventHandler>) {
        let id = self.next_handler_id;
        self.next_handler_id += 1;
        self.handlers.push((id, handler));
    }

    fn subscribe_filtered<F>(&mut self, filter: F, handler: Box<dyn EventHandler>)
    where
        F: Fn(&DomainEvent) -> bool + 'static,
    {
        let id = self.next_handler_id;
        self.next_handler_id += 1;

        struct FilteredHandler<F> {
            inner: Box<dyn EventHandler>,
            filter: F,
        }

        impl<F: Fn(&DomainEvent) -> bool> EventHandler for FilteredHandler<F> {
            fn handle(&self, event: &DomainEvent) {
                if (self.filter)(event) {
                    self.inner.handle(event);
                }
            }
        }

        self.handlers.push((id, Box::new(FilteredHandler { inner: handler, filter })));
    }
}
```

### 5.3 이벤트 필터링

```rust
// 특정 이벤트만 수신
event_bus.subscribe_filtered(
    |event| matches!(event, DomainEvent::AttackExecuted { .. }),
    Box::new(DamageLogHandler),
);

// 특정 캐릭터 관련 이벤트만 수신
event_bus.subscribe_filtered(
    |event| match event {
        DomainEvent::LevelUp { character_id, .. } => *character_id == target_id,
        DomainEvent::AttackExecuted { attacker_id, target_id, .. } =>
            *attacker_id == target_id || *target_id == target_id,
        _ => false,
    },
    Box::new(CharacterSpecificHandler),
);
```

### 5.4 이벤트 로깅

```rust
pub struct LoggingHandler {
    logger: Box<dyn Fn(&DomainEvent)>,
}

impl EventHandler for LoggingHandler {
    fn handle(&self, event: &DomainEvent) {
        (self.logger)(event);
    }
}

// 사용 예시
let log_handler = LoggingHandler {
    logger: Box::new(|event| {
        tracing::info!("DomainEvent: {:?}", event);
    }),
};
event_bus.subscribe(Box::new(log_handler));
```

---

## 6. 이벤트 기반 상태 동기화

### 6.1 현재 상태 동기화

현재 동기화 없음. 이벤트가 발생해도 외부에 알림 없음.

### 6.2 향후 동기화 설계

```rust
// 이벤트 기반 상태 동기화
pub struct StateSynchronizer {
    character_cache: HashMap<u64, CharacterSnapshot>,
    room_cache: HashMap<u32, RoomSnapshot>,
}

impl EventHandler for StateSynchronizer {
    fn handle(&self, event: &DomainEvent) {
        match event {
            DomainEvent::LevelUp { character_id, new_level } => {
                // 캐릭터 스냅샷 갱신
                if let Some(snapshot) = self.character_cache.get_mut(character_id) {
                    snapshot.level = *new_level;
                    snapshot.version += 1;
                }
            }
            DomainEvent::PlayerEnteredRoom { player_id, room_id } => {
                // 방 인원 업데이트
                if let Some(snapshot) = self.room_cache.get_mut(room_id) {
                    snapshot.player_ids.push(*player_id);
                    snapshot.version += 1;
                }
            }
            DomainEvent::ItemAcquired { player_id, item_id, quantity } => {
                // 인벤토리 스냅샷 갱신
                if let Some(snapshot) = self.character_cache.get_mut(player_id) {
                    snapshot.inventory_version += 1;
                }
            }
            _ => {}
        }
    }
}
```

### 6.3 클라이언트 푸시 알림

```rust
// WebSocket을 통한 클라이언트 알림
pub struct WebSocketPushHandler {
    connections: HashMap<u64, WebSocket>,
}

impl EventHandler for WebSocketPushHandler {
    fn handle(&self, event: &DomainEvent) {
        match event {
            DomainEvent::AttackExecuted { attacker_id, target_id, damage, .. } => {
                // 공격자에게
                if let Some(conn) = self.connections.get(attacker_id) {
                    let msg = PushMessage::AttackResult {
                        target: *target_id,
                        damage: *damage,
                    };
                    conn.send(msg);
                }
                // 대상에게
                if let Some(conn) = self.connections.get(target_id) {
                    let msg = PushMessage::DamageReceived {
                        attacker: *attacker_id,
                        damage: *damage,
                    };
                    conn.send(msg);
                }
            }
            _ => {}
        }
    }
}
```

---

## 7. 이벤트 소싱 (Event Sourcing) 가능성

### 7.1 개념

이벤트 소싱은 상태를 이벤트 히스토리로부터 재구성하는 패턴:

```
현재 상태 = replay(모든 이벤트)
```

### 7.2 장점

| 장점 | 설명 |
|------|------|
| **감사 추적** | 모든 변경 이력 기록 |
| **상태 복구** | 특정 시점의 상태로 복원 가능 |
| **디버깅** | 이벤트 리플레이를 통한 문제 재현 |
| **분석** | 플레이어 행동 패턴 분석 |

### 7.3 구현 설계

```rust
pub struct EventStore {
    events: Vec<DomainEvent>,
    snapshots: Vec<StateSnapshot>,
}

impl EventStore {
    pub fn append(&mut self, event: DomainEvent) {
        self.events.push(event);
    }

    // 특정 시점까지의 상태 재구성
    pub fn replay_until(&self, timestamp: u64) -> GameState {
        let mut state = GameState::default();
        for event in &self.events {
            if event.timestamp <= timestamp {
                state.apply(event);
            }
        }
        state
    }

    // 전체 이력 조회
    pub fn get_history(&self, entity_id: u64) -> Vec<&DomainEvent> {
        self.events.iter()
            .filter(|e| e.entity_id() == entity_id)
            .collect()
    }
}
```

### 7.4 스냅샷 최적화

이벤트가 너무 많아지면 스냅샷 사용:

```rust
impl EventStore {
    pub fn create_snapshot(&mut self, state: StateSnapshot) {
        self.snapshots.push(state);
    }

    pub fn replay_from_snapshot(&self, snapshot_id: usize) -> GameState {
        let snapshot = &self.snapshots[snapshot_id];
        let mut state = snapshot.state.clone();

        // 스냅샷 이후 이벤트만 리플레이
        for event in &self.events {
            if event.sequence > snapshot.last_sequence {
                state.apply(event);
            }
        }
        state
    }
}
```

---

## 8. 이벤트 직렬화/역직렬화

### 8.1 현재 구현

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEvent {
    CharacterCreated { character_id: u64, name: String },
    LevelUp { character_id: u64, new_level: u32 },
    // ...
}
```

`serde`를 통한 자동 직렬화/역직렬화 지원.

### 8.2 직렬화 포맷

| 포맷 | 용도 | 성능 | 비고 |
|------|------|------|------|
| JSON | 로깅, 디버깅 | 느림 | 사람-readable |
| MessagePack | 네트워크 전송 | 빠름 | 현재 사용 |
| Bincode | 내부 저장 | 매우 빠름 | 바이너리 |
| Protobuf | 외부 연동 | 빠름 | 스키마 필요 |

### 8.3 이벤트 직렬화 예시

```rust
// JSON 직렬화
let event = DomainEvent::AttackExecuted {
    combat_id: 1,
    attacker_id: 10,
    target_id: 20,
    damage: 15,
};
let json = serde_json::to_string(&event).unwrap();
// {"AttackExecuted":{"combat_id":1,"attacker_id":10,"target_id":20,"damage":15}}

// MessagePack 직렬화
let bytes = rmp_serde::to_vec(&event).unwrap();
// 바이너리 형태로 직렬화됨

// 역직렬화
let deserialized: DomainEvent = rmp_serde::from_slice(&bytes).unwrap();
```

### 8.4 이벤트 버전 관리

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedEvent {
    pub version: u32,       // 이벤트 스키마 버전
    pub sequence: u64,      // 이벤트 시퀀스 번호
    pub timestamp: u64,     // 발생 시점 (Unix timestamp)
    pub event: DomainEvent, // 실제 이벤트
}

impl VersionedEvent {
    pub fn new(event: DomainEvent, sequence: u64) -> Self {
        Self {
            version: 1,
            sequence,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            event,
        }
    }
}
```

---

## 9. 현재 구현 vs 미구현

### ✅ 구현 완료

| 기능 | 위치 | 상태 |
|------|------|------|
| DomainEvent 열거형 정의 | `domain/src/event.rs` | ✅ 완료 |
| 9개 이벤트 정의 | `DomainEvent` | ✅ 완료 |
| Serialize/Deserialize | `#[derive(Serialize, Deserialize)]` | ✅ 완료 |
| 이벤트 생성 (Character) | `character.rs` — `gain_experience()` | ✅ 완료 |
| 이벤트 생성 (Combat) | `combat.rs` — `process_attack()` | ✅ 완료 |

### ❌ 미구현

| 기능 | 우선순위 | 예상 작업량 |
|------|----------|-------------|
| 이벤트 버스 | 🔴 높음 | Large |
| 이벤트 핸들러 등록/발행 | 🔴 높음 | Medium |
| 이벤트 필터링 | 🟡 중간 | Medium |
| 이벤트 로깅 | 🟡 중간 | Small |
| 이벤트 기반 상태 동기화 | 🔴 높음 | Large |
| 이벤트 소싱 | 🟢 낮음 | Large |
| 이벤트 직렬화 (다중 포맷) | 🟢 낮음 | Medium |
| 이벤트 버전 관리 | 🟢 낮음 | Medium |
| PlayerEnteredRoom/LeftRoom | 🟡 중간 | Small |
| ItemAcquired/ItemRemoved | 🟡 중간 | Small |

---

## 10. 확장 고려사항

### 10.1 추가 이벤트 (설계)

```rust
pub enum DomainEvent {
    // 기존 9개 + 확장 이벤트
    // ...

    // 전투 확장
    DefendActivated { combat_id: u64, defender_id: u64 },
    CriticalHit { combat_id: u64, attacker_id: u64, damage: u32 },
    Dodged { combat_id: u64, defender_id: u64 },

    // 인벤토리 확장
    ItemUsed { player_id: u64, item_id: u32, effect: String },
    ItemEquipped { player_id: u64, item_id: u32, slot: String },
    ItemUnequipped { player_id: u64, item_id: u32, slot: String },

    // 월드 확장
    NpcSpawned { npc_id: u64, room_id: u32 },
    NpcDefeated { npc_id: u64, room_id: u32 },

    // 퀘스트 확장
    QuestStarted { player_id: u64, quest_id: u32 },
    QuestCompleted { player_id: u64, quest_id: u32 },

    // 퀘스트 확장
    QuestStarted { player_id: u64, quest_id: u32 },
    QuestCompleted { player_id: u64, quest_id: u32 },
}
```

### 10.2 이벤트 분석

이벤트 로그를 통한 게임 분석:

```rust
pub struct GameAnalytics {
    event_store: EventStore,
}

impl GameAnalytics {
    // 플레이어 행동 분석
    pub fn player_activity(&self, player_id: u64) -> PlayerStats {
        let events = self.event_store.get_history(player_id);
        PlayerStats {
            total_combats: events.iter().filter(|e| matches!(e, DomainEvent::CombatStarted { .. })).count(),
            total_kills: events.iter().filter(|e| matches!(e, DomainEvent::CombatEnded { .. })).count(),
            total_levels: events.iter().filter(|e| matches!(e, DomainEvent::LevelUp { .. })).count(),
            rooms_visited: self.count_unique_rooms(player_id),
        }
    }

    // 인기 방 분석
    pub fn popular_rooms(&self) -> Vec<(u32, usize)> {
        let mut room_counts: HashMap<u32, usize> = HashMap::new();
        for event in &self.event_store.events {
            if let DomainEvent::PlayerEnteredRoom { room_id, .. } = event {
                *room_counts.entry(*room_id).or_insert(0) += 1;
            }
        }
        let mut counts: Vec<_> = room_counts.into_iter().collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1));
        counts
    }
}
```
