# 월드/방 시스템 상세 설계

> 모듈: `domain::world`
> 소스: `domain/src/world.rs`

---

## 1. 월드 구조

### 1.1 전체 구조도

```
World
├─ rooms: HashMap<u32, Room>
│   ├─ Room (id: 1)
│   │   ├─ exits: {North → 2, East → 3, South → 4}
│   │   ├─ npc_ids: [1]
│   │   └─ item_ids: []
│   ├─ Room (id: 2)
│   │   ├─ exits: {South → 1, North → 5}
│   │   ├─ npc_ids: [2]
│   │   └─ item_ids: []
│   ├─ Room (id: 3)
│   │   ├─ exits: {West → 1}
│   │   ├─ npc_ids: [3]
│   │   └─ item_ids: [1, 2]
│   ├─ Room (id: 4)
│   │   ├─ exits: {North → 1}
│   │   ├─ npc_ids: []
│   │   └─ item_ids: [3, 4]
│   └─ Room (id: 5)
│       ├─ exits: {South → 2}
│       ├─ npc_ids: [4]
│       └─ item_ids: [5]
│
└─ npcs: HashMap<u64, Npc>
    ├─ Npc (id: 1) - Town Guard
    ├─ Npc (id: 2) - Forest Wolf
    ├─ Npc (id: 3) - Blacksmith Garen
    └─ Npc (id: 4) - Goblin
```

### 1.2 Room → Exit → Room 관계

```
┌──────────────┐     North      ┌──────────────┐     North      ┌──────────────┐
│              │ ──────────────▶ │              │ ──────────────▶ │              │
│ Town Square  │                 │ Forest Path  │                 │ Goblin Cave  │
│   (Room 1)   │ ◀──────────────│   (Room 2)   │ ◀──────────────│   (Room 5)   │
│              │     South      │              │     South      │              │
└──────────────┘                 └──────────────┘                 └──────────────┘
       │
       │ East                    ┌──────────────┐
       ▼                         │              │
┌──────────────┐     West       │              │
│              │ ◀──────────────│              │
│ Blacksmith   │                 │    Market    │
│   Shop (3)   │                 │   (Room 4)   │
│              │                 │              │
└──────────────┘                 └──────────────┘
                                       │
                                       │ North
                                       ▼
                                 ┌──────────────┐
                                 │ Town Square  │
                                 └──────────────┘
```

---

## 2. Room 엔티티

### 2.1 Room 구조체

```rust
pub struct Room {
    pub id: u32,                          // 방 고유 ID
    pub name: String,                     // 방 이름
    pub description: String,              // 방 설명 (50~200자)
    pub exits: HashMap<Direction, u32>,   // 출구 (방향 → 방 ID)
    pub npc_ids: Vec<u64>,               // 해당 방에 있는 NPC ID 목록
    pub item_ids: Vec<u32>,              // 해당 방에 있는 아이템 ID 목록
}
```

### 2.2 필드별 제약조건

| 필드 | 타입 | 제약조건 | 비고 |
|------|------|----------|------|
| `id` | `u32` | `> 0`, 유니크 | 방 생성 시 할당 |
| `name` | `String` | `1~64자` | UI 표시용 |
| `description` | `String` | `1~500자` | `look` 커맨드 시 표시 |
| `exits` | `HashMap<Direction, u32>` | 각 value는 유효한 Room ID | 최대 6방향 |
| `npc_ids` | `Vec<u64>` | 각 value는 유효한 Npc ID | NPCs 테이블 참조 |
| `item_ids` | `Vec<u32>` | 각 value는 유효한 Item ID | Items 테이블 참조 |

---

## 3. Direction 열거형

### 3.1 정의

```rust
pub enum Direction {
    North,  // 북
    South,  // 남
    East,   // 동
    West,   // 서
    Up,     // 위
    Down,   // 아래
}
```

### 3.2 입력 파싱

| 입력 | 변환 | 비고 |
|------|------|------|
| "north", "n" | `Direction::North` | 약어 지원 |
| "south", "s" | `Direction::South` | |
| "east", "e" | `Direction::East` | |
| "west", "w" | `Direction::West` | |
| "up", "u" | `Direction::Up` | |
| "down", "d" | `Direction::Down` | |

### 3.3 반대 방향

```rust
pub fn opposite(&self) -> Self {
    match self {
        Self::North => Self::South,
        Self::South => Self::North,
        Self::East => Self::West,
        Self::West => Self::East,
        Self::Up => Self::Down,
        Self::Down => Self::Up,
    }
}
```

---

## 4. 이동 메커니즘

### 4.1 이동 흐름

```
1. 플레이어: "north" 커맨드 입력
2. Application: move_character(character_id, Direction::North) 호출
3. Domain:
   ├─ 현재 방 조회 (character.room_id)
   ├─ 출구 존재 여부 확인 (room.exits.get(&Direction::North))
   ├─ 캐릭터 room_id 갱신
   └─ 새 방 정보 반환
4. Application: MoveResult 반환
5. Client: 새 방 표시
```

### 4.2 이동 처리 (application/src/service.rs)

```rust
pub fn move_character(
    &mut self,
    character_id: u64,
    direction: Direction,
) -> Result<MoveResult, ApplicationError> {
    let character = self.characters.get(&character_id)
        .ok_or(ApplicationError::CharacterNotFound(character_id))?;

    let current_room = self.world.get_room(character.room_id)
        .ok_or(ApplicationError::NoExit)?;

    let new_room_id = *current_room.exits.get(&direction)
        .ok_or(ApplicationError::NoExit)?;

    if let Some(character) = self.characters.get_mut(&character_id) {
        character.room_id = new_room_id;
    }

    let new_room = self.world.get_room(new_room_id)
        .ok_or(ApplicationError::NoExit)?;

    Ok(MoveResult {
        from_room_id: current_room_id,
        to_room_id: new_room_id,
        room_name: new_room.name.clone(),
        room_description: new_room.description.clone(),
    })
}
```

### 4.3 이동 제한 (설계)

| 제한 | 처리 | 비고 |
|------|------|------|
| 해당 방향 출구 없음 | `ApplicationError::NoExit` | 현재 구현 |
| 전투 중 이동 불가 | 검증 필요 | 미구현 |
| 막힌 길 | `ExitCondition::Blocked` | 미구현 |
| 레벨 제한 | `ExitCondition::LevelRequired(n)` | 미구현 |
| 아이템 필요 | `ExitCondition::ItemRequired(id)` | 미구현 |

---

## 5. 현재 월드 맵 (5개 방)

### 5.1 Town Square (시작 방)

| 항목 | 내용 |
|------|------|
| **ID** | 1 |
| **이름** | Town Square |
| **설명** | "A bustling town square with a fountain in the center. The water sparkles in the sunlight." |
| **출구** | North → Room 2 (Forest Path), East → Room 3 (Blacksmith Shop), South → Room 4 (Market) |
| **NPC** | Town Guard (ID: 1) |
| **아이템** | 없음 |
| **특징** | 시작 방, 가장 많은 출구 (3개) |

### 5.2 Forest Path

| 항목 | 내용 |
|------|------|
| **ID** | 2 |
| **이름** | Forest Path |
| **설명** | "A winding path through a dense forest. Birds chirp in the canopy above." |
| **출구** | South → Room 1 (Town Square), North → Room 5 (Goblin Cave) |
| **NPC** | Forest Wolf (ID: 2) |
| **아이템** | 없음 |
| **특징** | Town과 Goblin Cave 사이 연결 통로 |

### 5.3 Blacksmith Shop

| 항목 | 내용 |
|------|------|
| **ID** | 3 |
| **이름** | Blacksmith Shop |
| **설명** | "The rhythmic clang of hammer on anvil fills this dimly lit shop. Weapons and armor line the walls." |
| **출구** | West → Room 1 (Town Square) |
| **NPC** | Blacksmith Garen (ID: 3) |
| **아이템** | 낡은 검 (ID: 1), 가죽 갑옷 (ID: 2) |
| **특징** | 상점 기능, 장비 구매 가능 |

### 5.4 Market

| 항목 | 내용 |
|------|------|
| **ID** | 4 |
| **이름** | Market |
| **설명** | "A lively market with colorful stalls. Merchants call out to passersby." |
| **출구** | North → Room 1 (Town Square) |
| **NPC** | 없음 |
| **아이템** | 체력 포션 (ID: 3), 마나 포션 (ID: 4) |
| **특징** | 소모품 구매 가능 |

### 5.5 Goblin Cave

| 항목 | 내용 |
|------|------|
| **ID** | 5 |
| **이름** | Goblin Cave |
| **설명** | "A dark, damp cave. The sound of dripping water echoes through the darkness. Something moves in the shadows." |
| **출구** | South → Room 2 (Forest Path) |
| **NPC** | Goblin (ID: 4) |
| **아이템** | 고블린 이빨 (ID: 5) |
| **특징** | 전투 구역, 난이도 높음 |

---

## 6. 월드 확장 설계

### 6.1 방 추가 방법

#### 6.1.1 코드 기반 추가 (현재 방식)

```rust
// world.rs의 initialize() 함수에 직접 추가
self.rooms.insert(6, Room {
    id: 6,
    name: "Mountain Pass".to_string(),
    description: "A narrow path winding up the mountain...".to_string(),
    exits: {
        let mut exits = HashMap::new();
        exits.insert(Direction::South, 2);
        exits.insert(Direction::Up, 7);
        exits
    },
    npc_ids: vec![5],
    item_ids: vec![],
});
```

#### 6.1.2 JSON/YAML 파일 기반 (설계)

```json
{
  "id": 6,
  "name": "Mountain Pass",
  "description": "A narrow path winding up the mountain...",
  "exits": {
    "South": 2,
    "Up": 7
  },
  "npc_ids": [5],
  "item_ids": []
}
```

#### 6.1.3 데이터베이스 기반 (설계)

```sql
CREATE TABLE rooms (
    id INTEGER PRIMARY KEY,
    name VARCHAR(64) NOT NULL,
    description TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE room_exits (
    room_id INTEGER REFERENCES rooms(id),
    direction VARCHAR(10) NOT NULL,
    target_room_id INTEGER REFERENCES rooms(id),
    condition_type VARCHAR(20),  -- NULL, 'level', 'item', 'key'
    condition_value INTEGER,
    PRIMARY KEY (room_id, direction)
);

CREATE TABLE room_npcs (
    room_id INTEGER REFERENCES rooms(id),
    npc_id BIGINT REFERENCES npcs(id),
    PRIMARY KEY (room_id, npc_id)
);
```

### 6.2 맵 에디터 설계 (설계)

#### 6.2.1 CLI 기반 에디터

```bash
# 방 추가
$ map-editor add-room --name "Dark Forest" --description "A dark forest..."

# 출구 추가
$ map-editor add-exit --from 1 --to 6 --direction north

# NPC 배치
$ map-editor place-npc --room 6 --npc-id 5

# 아이템 배치
$ map-editor place-item --room 6 --item-id 10

# 현재 맵 시각화
$ map-editor visualize
```

#### 6.2.2 웹 기반 에디터 (설계)

- 드래그 앤 드롭으로 방 배치
- 연결선으로 출구 설정
- 실시간 미리보기
- JSON 내보내기/가져오기

### 6.3 동적 방 로딩 (설계)

현재 모든 방이 메모리에 로드됨. 대규모 월드 확장 시:

```rust
pub struct DynamicWorld {
    loaded_rooms: HashMap<u32, Room>,    // 현재 로드된 방
    room_index: HashMap<u32, String>,    // 방 ID → 파일 경로 매핑
    max_loaded: usize,                   // 최대 로드 방 수
}

impl DynamicWorld {
    pub fn get_room(&mut self, room_id: u32) -> Option<&Room> {
        if let Some(room) = self.loaded_rooms.get(&room_id) {
            return Some(room);
        }

        // 디스크에서 로드
        let path = self.room_index.get(&room_id)?;
        let room = self.load_from_disk(path).ok()?;
        self.loaded_rooms.insert(room_id, room);

        // LRU 캐시 관리
        if self.loaded_rooms.len() > self.max_loaded {
            self.evict_oldest();
        }

        self.loaded_rooms.get(&room_id)
    }
}
```

---

## 7. NPC 시스템

### 7.1 NPC 엔티티 구조

```rust
pub struct Npc {
    pub id: u64,          // NPC 고유 ID
    pub name: String,     // NPC 이름
    pub description: String, // NPC 설명
    pub room_id: u32,     // 현재 위치
    pub hp: u32,          // 현재 HP
    pub max_hp: u32,      // 최대 HP
}
```

### 7.2 NPC 종류

| ID | 이름 | 방 ID | HP | 유형 | 비고 |
|----|------|-------|----|------|------|
| 1 | Town Guard | 1 | 100/100 | 친선 | 전투 불가 |
| 2 | Forest Wolf | 2 | 50/50 | 적대 | 전투 가능 |
| 3 | Blacksmith Garen | 3 | 120/120 | 친선 | 상점 기능 |
| 4 | Goblin | 5 | 30/30 | 적대 | 전투 가능 |

### 7.3 NPC 타입 (설계)

```rust
pub enum NpcType {
    Friendly,   // 친선 — 공격 불가, 대화/상점 가능
    Hostile,    // 적대 — 전투 가능
    Neutral,    // 중립 — 특정 조건 시 적대
}
```

### 7.4 NPC 대화 시스템 (설계)

```rust
pub struct DialogueTree {
    pub id: u32,
    pub npc_id: u64,
    pub nodes: Vec<DialogueNode>,
}

pub struct DialogueNode {
    pub id: u32,
    pub text: String,                    // NPC 대사
    pub options: Vec<DialogueOption>,    // 선택지
}

pub struct DialogueOption {
    pub text: String,                    // 선택지 텍스트
    pub next_node: Option<u32>,          // 다음 노드 ID (None이면 대화 종료)
    pub action: Option<DialogueAction>,  // 선택 시 실행할 액션
}

pub enum DialogueAction {
    GiveItem(u32),           // 아이템 지급
    TakeItem(u32),           // 아이템 회수
    StartQuest(u32),         // 퀘스트 시작
    CompleteQuest(u32),      // 퀘스트 완료
    OpenShop,                // 상점 열기
    StartCombat,             // 전투 시작
}
```

**대화 예시 (Town Guard):**

```
Town Guard: "Welcome to the town. Be careful in the forest."

[1] "What's in the forest?" → 다음 노드
[2] "Do you have any quests?" → 퀘스트 목록
[3] "Goodbye." → 대화 종료
```

### 7.5 NPC 상점 (설계)

```rust
pub struct Shop {
    pub npc_id: u64,
    pub inventory: Vec<ShopItem>,
    pub buy_rate: f64,   // 구매 배율 (1.0 = 정가)
    pub sell_rate: f64,  // 판매 배율 (0.5 = 절반가)
}

pub struct ShopItem {
    pub item_id: u32,
    pub price: u64,          // 구매 가격
    pub stock: Option<u32>,  // None이면 무제한
}
```

**상점 예시 (Blacksmith Garen):**

| 아이템 | 구매 가격 | 재고 |
|--------|-----------|------|
| 낡은 검 | 50g | 무제한 |
| 가죽 갑옷 | 30g | 무제한 |
| 철 투구 | 80g | 5개 |
| 강철 갑옷 | 150g | 3개 |

### 7.6 NPC 리스폰 (설계)

```rust
pub struct RespawnConfig {
    pub respawn_time: Duration,     // 리스폰 대기 시간
    pub respawn_hp: f64,           // 리스폰 시 HP 비율 (1.0 = 풀 HP)
    pub max_respawn_count: Option<u32>, // 최대 리스폰 횟수 (None이면 무제한)
}

impl Npc {
    pub fn schedule_respawn(&self) -> Option<RespawnEvent> {
        if self.npc_type != NpcType::Hostile {
            return None;
        }

        Some(RespawnEvent {
            npc_id: self.id,
            room_id: self.spawn_room_id,  // 원래 생성 방
            delay: Duration::from_secs(300),  // 5분
            hp_ratio: 1.0,
        })
    }
}
```

---

## 8. 아이템 월드 배치

### 8.1 현재 배치 상태

| 방 ID | 아이템 ID | 아이템 이름 | 비고 |
|-------|-----------|-------------|------|
| 3 (Blacksmith Shop) | 1 | 낡은 검 | 상점 아이템 |
| 3 (Blacksmith Shop) | 2 | 가죽 갑옷 | 상점 아이템 |
| 4 (Market) | 3 | 체력 포션 | 상점 아이템 |
| 4 (Market) | 4 | 마나 포션 | 상점 아이템 |
| 5 (Goblin Cave) | 5 | 고블린 이빨 | 전리품 |

### 8.2 월드 아이템 타입 (설계)

```rust
pub enum WorldItemType {
    Static,    // 고정 — 항상 같은 위치
    Spawn,     // 스폰 — 리스폰됨
    Pickup,    // 줍기 — 플레이어가 줍으면 사라짐
    Quest,     // 퀘스트 — 특정 퀘스트 관련
}
```

### 8.3 아이템 스폰 시스템 (설계)

```rust
pub struct ItemSpawn {
    pub item_id: u32,
    pub room_id: u32,
    pub respawn_time: Option<Duration>,  // None이면 리스폰 없음
    pub max_count: u32,                  // 최대 동시 존재 수량
}

impl World {
    pub fn spawn_item(&mut self, spawn: &ItemSpawn) {
        let room = self.rooms.get_mut(&spawn.room_id).unwrap();
        let current_count = room.item_ids.iter().filter(|&&id| id == spawn.item_id).count();

        if current_count < spawn.max_count as usize {
            room.item_ids.push(spawn.item_id);
        }
    }
}
```

---

## 9. 현재 구현 vs 미구현

### ✅ 구현 완료

| 기능 | 위치 | 상태 |
|------|------|------|
| Room 엔티티 | `Room` | ✅ 완료 |
| Direction 열거형 | `Direction` | ✅ 완료 |
| 방향 파싱 | `Direction::from_str()` | ✅ 완료 |
| 반대 방향 | `Direction::opposite()` | ✅ 완료 |
| World 구조체 | `World` | ✅ 완료 |
| 방 조회 | `World::get_room()` | ✅ 완료 |
| NPC 조회 | `World::get_npc()` | ✅ 완료 |
| 방 내 NPC 검색 | `World::find_npc_in_room()` | ✅ 완료 |
| 기본 월드 생성 | `World::initialize()` | ✅ 완료 |
| 5개 방 생성 | `initialize()` 내 | ✅ 완료 |
| 4개 NPC 생성 | `initialize()` 내 | ✅ 완료 |
| 이동 메커니즘 | `GameWorld::move_character()` | ✅ 완료 |
| 방 정보 조회 | `GameWorld::look_room()` | ✅ 완료 |

### ❌ 미구현

| 기능 | 우선순위 | 예상 작업량 |
|------|----------|-------------|
| NPC 대화 시스템 | 🔴 높음 | Large |
| NPC 상점 | 🔴 높음 | Medium |
| NPC 리스폰 | 🟡 중간 | Medium |
| 방/출구 조건부 | 🟡 중간 | Medium |
| 동적 방 로딩 | 🟢 낮음 | Large |
| JSON 기반 방 정의 | 🟡 중간 | Medium |
| 아이템 월드 배치 | 🟢 낮음 | Small |
| 방 이벤트 (트리거) | 🟡 중간 | Medium |
| 멀티플레이어 동기화 | 🔴 높음 | Large |

---

## 10. 확장 고려사항

### 10.1 퀘스트 시스템 연동

방 진입 시 퀘스트 트리거:

```rust
pub struct RoomTrigger {
    pub event_type: TriggerType,
    pub condition: TriggerCondition,
    pub action: TriggerAction,
}

pub enum TriggerType {
    OnEnter,      // 방 진입 시
    OnExit,       // 방 탈출 시
    OnLook,       // 방 관찰 시
    OnInteract,   // 상호작용 시
}
```

### 10.2 방 상호작용

방 내 오브젝트와의 상호작용:

```
> look fountain
You see a beautiful fountain with sparkling water.

> interact fountain
You drink from the fountain. HP fully restored!
```

### 10.3 시간 기반 월드 변화

```rust
pub struct TimeSystem {
    pub current_time: GameTime,
    pub time_scale: f64,  // 1.0 = 실시간, 60.0 = 1분 = 1시간
}

pub enum GameTime {
    Dawn,    // 새벽
    Day,     // 낮
    Dusk,    // 저녁
    Night,   // 밤
}

// 밤에만 등장하는 몬스터, 낮에만 열리는 상점 등
```
