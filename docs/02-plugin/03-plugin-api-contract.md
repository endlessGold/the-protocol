# 플러그인 API 컨트랙트 명세

> The Protocol 플러그인과 런타임 간 API 인터페이스 전체 명세

## 1. 개요

이 문서는 플러그인이 런타임과 통신하기 위한 API 컨트랙트를 정의합니다. Host Functions(런타임 → 플러그인)와 Plugin Exports(플러그인 → 런타임)의 전체 인터페이스를 다룹니다.

## 2. 통신 모델

```
┌─────────────────────────────────────────────────────┐
│                    Runtime (Host)                    │
│                                                      │
│  Host Functions:                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │ logging, storage, events, timers,            │   │
│  │ player, inventory, combat, communication     │   │
│  └──────────────────────────────────────────────┘   │
│         ↕                                            │
│  ┌──────────────────────────────────────────────┐   │
│  │              WASM Interface                   │   │
│  │  - allocate_buffer(size) -> buffer_id        │   │
│  │  - read_buffer(buffer_id) -> ptr             │   │
│  │  - write_buffer(buffer_id, ptr, len)         │   │
│  │  - free_buffer(buffer_id)                    │   │
│  └──────────────────────────────────────────────┘   │
│         ↕                                            │
│  Plugin Exports:                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │ plugin_init, plugin_enable, plugin_disable,  │   │
│  │ plugin_unload, handle_command, handle_event, │   │
│  │ handle_timer                                 │   │
│  └──────────────────────────────────────────────┘   │
│                                                      │
└─────────────────────────────────────────────────────┘
```

## 3. Host Functions (Runtime → Plugin) 전체 인터페이스

### 3.1 함수 시그니처

| 함수명 | 파라미터 | 반환값 | 설명 |
|--------|---------|--------|------|
| `log` | `(level: i32, ptr: u32, len: u32)` | `()` | 로깅 |
| `storage_get` | `(key_ptr: u32, key_len: u32)` | `i64` | 스토리지 읽기 |
| `storage_set` | `(key_ptr: u32, key_len: u32, val_ptr: u32, val_len: u32)` | `i32` | 스토리지 쓰기 |
| `storage_delete` | `(key_ptr: u32, key_len: u32)` | `i32` | 스토리지 삭제 |
| `emit_event` | `(type_ptr: u32, type_len: u32, data_ptr: u32, data_len: u32)` | `i32` | 이벤트 발행 |
| `set_timer` | `(delay_ms: i64, repeat: i32, callback_id: i32)` | `i64` | 타이머 설정 |
| `cancel_timer` | `(timer_id: i64)` | `i32` | 타이머 취소 |
| `player_get` | `(player_id: i64)` | `i64` | 플레이어 데이터 조회 |
| `player_update` | `(player_id: i64, data_ptr: u32, data_len: u32)` | `i32` | 플레이어 데이터 수정 |
| `inventory_get` | `(player_id: i64)` | `i64` | 인벤토리 조회 |
| `inventory_add_item` | `(player_id: i64, item_id: i64, count: i32)` | `i32` | 아이템 추가 |
| `inventory_remove_item` | `(player_id: i64, item_id: i64, count: i32)` | `i32` | 아이템 제거 |
| `combat_start` | `(attacker_id: i64, defender_id: i64)` | `i64` | 전투 시작 |
| `combat_action` | `(combat_id: i64, action_ptr: u32, action_len: u32)` | `i32` | 전투 행동 실행 |
| `send_to_client` | `(player_id: i64, msg_ptr: u32, msg_len: u32)` | `i32` | 클라이언트에 메시지 전송 |
| `broadcast_to_room` | `(room_id: i64, msg_ptr: u32, msg_len: u32)` | `i32` | 룸 전체에 브로드캐스트 |

### 3.2 버퍼 관리 함수

| 함수명 | 파라미터 | 반환값 | 설명 |
|--------|---------|--------|------|
| `allocate_buffer` | `(size: u32)` | `i64` | 버퍼 할당, buffer_id 반환 |
| `read_buffer` | `(buffer_id: i64, dst_ptr: u32, max_len: u32)` | `i32` | 버퍼에서 데이터 읽기 |
| `write_buffer` | `(buffer_id: i64, src_ptr: u32, src_len: u32)` | `i32` | 버퍼에 데이터 쓰기 |
| `free_buffer` | `(buffer_id: i64)` | `()` | 버퍼 해제 |

### 3.3 반환 코드 체계

#### 공통 반환 코드

| 코드 | 의미 |
|------|------|
| `0` | 성공 |
| `-1` | 일반 에러 |
| `-2` | 리소스 미발견 (NOT_FOUND) |
| `-3` | 스토리지 에러 |
| `-4` | 직렬화 에러 |
| `-5` | 권한 에러 |
| `-6` | 리소스 초과 |
| `-7` | 타임아웃 |
| `-8` | 잘못된 파라미터 |

#### 함수별 특수 반환 코드

| 함수 | 코드 | 의미 |
|------|------|------|
| `storage_get` | `-10` | 키가 유효하지 않음 |
| `player_get` | `-20` | 플레이어가 존재하지 않음 |
| `combat_start` | `-30` | 전투를 시작할 수 없음 |
| `combat_action` | `-31` | 유효하지 않은 전투 행동 |

## 4. Plugin Exports (Plugin → Runtime) 전체 인터페이스

### 4.1 생명주기 Export

```wat
;; plugin_init: 플러그인 초기화
;; 호출 시점: WASM 모듈 인스턴스화 후 최초 1회
;; 반환: 0=성공, 비-0=실패
(func (export "plugin_init") (result i32))

;; plugin_enable: 플러그인 활성화
;; 호출 시점: plugin_init 성공 후
;; 반환: 0=성공
(func (export "plugin_enable") (result i32))

;; plugin_disable: 플러그인 비활성화
;; 호출 시점: 명령 또는 의존성 플러그인 비활성화 시
;; 반환: 0=성공
(func (export "plugin_disable") (result i32))

;; plugin_unload: 플러그인 언로드
;; 호출 시점: 플러그인 제거 시
;; 반환: 0=성공
(func (export "plugin_unload") (result i32))
```

### 4.2 명령어 핸들러 Export

```wat
;; handle_command: 명령어 처리
;; 파라미터:
;;   command_ptr: 명령어 문자열 포인터
;;   command_len: 명령어 문자열 길이
;;   args_ptr: 인자 JSON 문자열 포인터
;;   args_len: 인자 JSON 문자열 길이
;;   player_id: 명령을 실행하는 플레이어 ID
;; 반환: 0=성공, 비-0=실패
(func (export "handle_command")
    (param $command_ptr i32) (param $command_len i32)
    (param $args_ptr i32) (param $args_len i32)
    (param $player_id i64)
    (result i32))
```

### 4.3 이벤트 핸들러 Export

```wat
;; handle_event: 이벤트 처리
;; 파라미터:
;;   event_type_ptr: 이벤트 타입 문자열 포인터
;;   event_type_len: 이벤트 타입 문자열 길이
;;   data_ptr: 이벤트 데이터 (MessagePack) 포인터
;;   data_len: 이벤트 데이터 길이
;; 반환: 0=성공, 비-0=실패
(func (export "handle_event")
    (param $event_type_ptr i32) (param $event_type_len i32)
    (param $data_ptr i32) (param $data_len i32)
    (result i32))
```

### 4.4 타이머 콜백 Export

```wat
;; handle_timer: 타이머 콜백 처리
;; 파라미터:
;;   timer_id: 타이머 ID
;;   callback_id: 콜백 ID (set_timer 시 지정)
;; 반환: 0=성공, 비-0=실패
(func (export "handle_timer")
    (param $timer_id i64) (param $callback_id i32)
    (result i32))
```

## 5. MessagePack 기반 직렬화 명세

### 5.1 데이터 교환 포맷

모든 복잡한 데이터 구조는 MessagePack 형식으로 직렬화됩니다.

### 5.2 PlayerData 구조체

```rust
#[derive(Serialize, Deserialize)]
pub struct PlayerData {
    pub id: i64,
    pub name: String,
    pub level: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub position: Position,
    pub stats: Stats,
    pub status_effects: Vec<StatusEffect>,
}

#[derive(Serialize, Deserialize)]
pub struct Position {
    pub room_id: i64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Serialize, Deserialize)]
pub struct Stats {
    pub strength: i32,
    pub dexterity: i32,
    pub intelligence: i32,
    pub constitution: i32,
    pub wisdom: i32,
    pub charisma: i32,
}

#[derive(Serialize, Deserialize)]
pub struct StatusEffect {
    pub effect_type: String,
    pub duration: i32,
    pub magnitude: f64,
}
```

### 5.3 PlayerUpdate 구조체

```rust
#[derive(Serialize, Deserialize)]
pub struct PlayerUpdate {
    pub hp: Option<i32>,
    pub mp: Option<i32>,
    pub position: Option<Position>,
    pub stats: Option<Stats>,
    pub status_effects: Option<Vec<StatusEffect>>,
    pub remove_status_effects: Option<Vec<String>>,
}
```

### 5.4 InventoryData 구조체

```rust
#[derive(Serialize, Deserialize)]
pub struct InventoryData {
    pub items: Vec<InventorySlot>,
    pub gold: i64,
    pub weight: f64,
    pub max_weight: f64,
}

#[derive(Serialize, Deserialize)]
pub struct InventorySlot {
    pub slot_index: i32,
    pub item_id: i64,
    pub item_name: String,
    pub count: i32,
    pub properties: HashMap<String, String>,
}
```

### 5.5 CombatAction 구조체

```rust
#[derive(Serialize, Deserialize)]
pub struct CombatAction {
    pub action_type: CombatActionType,
    pub target_id: Option<i64>,
    pub item_id: Option<i64>,
    pub skill_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub enum CombatActionType {
    Attack,
    Defend,
    UseItem,
    UseSkill,
    Flee,
}
```

### 5.6 CombatResult 구조체

```rust
#[derive(Serialize, Deserialize)]
pub struct CombatResult {
    pub success: bool,
    pub damage: Option<i32>,
    pub healing: Option<i32>,
    pub status_effects: Vec<StatusEffect>,
    pub message: String,
    pub combat_ended: bool,
    pub winner: Option<i64>,
}
```

### 5.7 PluginEvent 구조체

```rust
#[derive(Serialize, Deserialize)]
pub struct PluginEvent {
    pub source: String,
    pub event_type: String,
    pub data: Vec<u8>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
```

### 5.8 이벤트 타입 목록

| 이벤트 타입 | 설명 | 데이터 형식 |
|-------------|------|-------------|
| `player.login` | 플레이어 로그인 | PlayerData |
| `player.logout` | 플레이어 로그아웃 | PlayerData |
| `player.move` | 플레이어 이동 | Position |
| `player.chat` | 채팅 메시지 | ChatMessage |
| `combat.start` | 전투 시작 | CombatStartData |
| `combat.action` | 전투 행동 | CombatAction |
| `combat.end` | 전투 종료 | CombatResult |
| `item.pickup` | 아이템 줍기 | InventorySlot |
| `item.drop` | 아이템 드롭 | InventorySlot |
| `item.use` | 아이템 사용 | UseItemData |
| `room.enter` | 룸 입장 | RoomData |
| `room.exit` | 룸 퇴장 | RoomData |
| `timer.tick` | 타이머 틱 | TimerTickData |
| `server.start` | 서버 시작 | ServerStartData |
| `server.stop` | 서버 종료 | () |

## 6. Buffer Management 상세

### 6.1 할당/읽기/쓰기/해제 프로토콜

```
Step 1: Allocate
  Plugin → Host: allocate_buffer(size)
  Host → Plugin: buffer_id (i64)

Step 2: Write (Plugin → Host)
  Plugin: 데이터를 Linear Memory에 기록
  Plugin → Host: write_buffer(buffer_id, ptr, len)
  Host → Plugin: 0 (성공)

Step 3: Read (Host → Plugin)
  Host → Plugin: read_buffer(buffer_id, dst_ptr, max_len)
  Plugin: dst_ptr에서 데이터 읽기
  Plugin → Host: 읽은 바이트 수 (i32)

Step 4: Free
  Plugin → Host: free_buffer(buffer_id)
  Host: 버퍼 해제
```

### 6.2 버퍼 수명 관리

- 버퍼는 명시적으로 `free_buffer`를 호출할 때까지 유효
- 플러그인 종료 시 모든 버퍼 자동 해제
- 최대 동시 버퍼 수: 1,000개
- 최대 버퍼 크기: 16MB

### 6.3 에러 시나리오

| 시나리오 | 처리 |
|---------|------|
| 유효하지 않은 buffer_id | `-1` 반환 |
| buffer_id 이미 해제 | `-1` 반환 |
| max_len 초과 | 읽을 수 있는 최대 바이트 수 반환 |
| 버퍼 할당 실패 (메모리 부족) | `-6` 반환 |

## 7. 에러 코드 체계

### 7.1 에러 분류

```rust
pub enum ApiError {
    // 공통 에러 (0 ~ -9)
    Success = 0,
    GeneralError = -1,
    NotFound = -2,
    StorageError = -3,
    SerializationError = -4,
    PermissionDenied = -5,
    ResourceExceeded = -6,
    Timeout = -7,
    InvalidParameter = -8,
    NotImplemented = -9,

    // 스토리지 에러 (-10 ~ -19)
    InvalidKey = -10,
    KeyTooLong = -11,
    ValueTooLong = -12,
    StorageFull = -13,

    // 플레이어 에러 (-20 ~ -29)
    PlayerNotFound = -20,
    PlayerOffline = -21,
    PlayerDead = -22,
    PlayerBusy = -23,

    // 인벤토리 에러 (-30 ~ -39)
    InventoryFull = -30,
    ItemNotFound = -31,
    InsufficientCount = -32,
    ItemNotUsable = -33,

    // 전투 에러 (-40 ~ -49)
    CombatNotActive = -40,
    InvalidAction = -41,
    TargetNotFound = -42,
    CooldownActive = -43,

    // 통신 에러 (-50 ~ -59)
    MessageTooLong = -50,
    PlayerDisconnected = -51,
    RoomNotFound = -52,
}
```

### 7.2 에러 메시지 전달

에러 발생 시 플러그인은 `log` Host Function을 통해 상세 에러 메시지를 전달할 수 있습니다.

## 8. API 버전 관리

### 8.1 버전 형식

```
MAJOR.MINOR

MAJOR: 호환되지 않는 변경사항
MINOR: 하위 호환되는 기능 추가
```

### 8.2 현재 API 버전

```
API Version: 1.0
```

### 8.3 버전 호환성 규칙

| 시나리오 | 처리 |
|---------|------|
| 플러그인 MAJOR < 런타임 MAJOR | 로딩 거부 |
| 플러그인 MAJOR = 런타임 MAJOR, MINOR ≤ 런타임 MINOR | 허용 |
| 플러그인 MAJOR = 런타임 MAJOR, MINOR > 런타임 MINOR | 경고 후 허용 |
| 플러그인 MAJOR > 런타임 MAJOR | 로딩 거부 |

### 8.4 버전 검증 로직

```rust
pub fn validate_api_version(
    plugin_version: &str,
    runtime_version: &str,
) -> Result<(), PluginError> {
    let plugin_parts: Vec<&str> = plugin_version.split('.').collect();
    let runtime_parts: Vec<&str> = runtime_version.split('.').collect();

    let plugin_major: u32 = plugin_parts[0].parse().unwrap_or(0);
    let runtime_major: u32 = runtime_parts[0].parse().unwrap_or(0);

    if plugin_major != runtime_major {
        return Err(PluginError::IncompatibleApiVersion {
            plugin: plugin_version.to_string(),
            required: runtime_version.to_string(),
        });
    }

    Ok(())
}
```

## 9. TypeScript 플러그인 예시 코드

### 9.1 매니페스트 (plugin.toml)

```toml
name = "combat-system"
version = "1.0.0"
description = "전투 시스템 플러그인"
api_version = "1.0"

[permissions]
required = ["player.read", "player.write", "combat.start", "combat.action"]

[resources]
memory_limit = "32MB"
execution_limit = 2_000_000

[dependencies]
"inventory-system" = ">=1.0.0"
```

### 9.2 TypeScript 플러그인 코드

```typescript
// combat-system/index.ts
import {
  plugin_init,
  plugin_enable,
  plugin_disable,
  plugin_unload,
  handle_command,
  handle_event,
  handle_timer,
} from "@the-protocol/sdk";

import {
  log,
  player_get,
  player_update,
  inventory_get,
  inventory_add_item,
  inventory_remove_item,
  combat_start,
  combat_action,
  send_to_client,
  emit_event,
  set_timer,
} from "@the-protocol/host";

// 상태 관리
let combatTimers: Map<number, number> = new Map();

// 생명주기
export function init(): number {
  log(2, "Combat system initialized");
  return 0; // 성공
}

export function enable(): number {
  log(2, "Combat system enabled");
  return 0;
}

export function disable(): number {
  log(2, "Combat system disabled");
  // 실행 중인 전투 타이머 정리
  combatTimers.forEach((timerId) => {
    cancel_timer(timerId);
  });
  combatTimers.clear();
  return 0;
}

export function unload(): number {
  log(2, "Combat system unloaded");
  return 0;
}

// 명령어 핸들러
export function handle_command(
  command: string,
  args: string,
  player_id: number
): number {
  switch (command) {
    case "attack":
      return handle_attack(args, player_id);
    case "defend":
      return handle_defend(args, player_id);
    case "flee":
      return handle_flee(args, player_id);
    default:
      return -1; // 알 수 없는 명령어
  }
}

function handle_attack(args: string, player_id: number): number {
  const target_id = parse_target_id(args);
  if (target_id === null) {
    send_to_client(player_id, "Usage: attack <target>");
    return -8; // InvalidParameter
  }

  // 플레이어 데이터 조회
  const player_data = player_get(player_id);
  if (player_data === null) {
    return -20; // PlayerNotFound
  }

  // 전투 시작
  const combat_id = combat_start(player_id, target_id);
  if (combat_id < 0) {
    send_to_client(player_id, "Cannot start combat");
    return combat_id;
  }

  // 공격 실행
  const action = {
    action_type: "Attack",
    target_id: target_id,
  };
  const result = combat_action(combat_id, action);

  if (result === 0) {
    emit_event("combat.action", { combat_id, action: "attack" });
  }

  return result;
}

function handle_defend(args: string, player_id: number): number {
  // 방어 로직 구현
  return 0;
}

function handle_flee(args: string, player_id: number): number {
  // 도망 로직 구현
  return 0;
}

// 이벤트 핸들러
export function handle_event(event_type: string, data: ArrayBuffer): number {
  switch (event_type) {
    case "player.login":
      return handle_player_login(data);
    case "player.logout":
      return handle_player_logout(data);
    default:
      return 0; // 이벤트 무시
  }
}

function handle_player_login(data: ArrayBuffer): number {
  // 로그인 시 초기화 작업
  return 0;
}

function handle_player_logout(data: ArrayBuffer): number {
  // 로그아웃 시 전투 정리
  return 0;
}

// 타이머 콜백
export function handle_timer(timer_id: number, callback_id: number): number {
  // 전투 틱 처리
  return 0;
}
```

## 10. C# 플러그인 예시 코드 (개념)

### 10.1 C# 플러그인 클래스

```csharp
using TheProtocol.Sdk;
using TheProtocol.Sdk.Attributes;
using TheProtocol.Sdk.Interfaces;

namespace CombatSystem;

[Plugin(
    Name = "combat-system",
    Version = "1.0.0",
    ApiVersion = "1.0",
    Description = "전투 시스템 플러그인"
)]
[RequiredPermissions(
    "player.read",
    "player.write",
    "combat.start",
    "combat.action"
)]
[MemoryLimit("32MB")]
[ExecutionLimit(2_000_000)]
[Dependency("inventory-system", ">=1.0.0")]
public class CombatPlugin : IPlugin
{
    private readonly IHostFunctions _host;
    private readonly ILogger _logger;

    public CombatPlugin(IHostFunctions host, ILogger logger)
    {
        _host = host;
        _logger = logger;
    }

    public int Initialize()
    {
        _logger.Info("Combat system initialized");
        return 0;
    }

    public int Enable()
    {
        _logger.Info("Combat system enabled");
        return 0;
    }

    public int Disable()
    {
        _logger.Info("Combat system disabled");
        return 0;
    }

    public int Unload()
    {
        _logger.Info("Combat system unloaded");
        return 0;
    }

    [CommandHandler("attack")]
    public int HandleAttack(string args, long playerId)
    {
        var targetId = ParseTargetId(args);
        if (targetId == null)
        {
            _host.SendToClient(playerId, "Usage: attack <target>");
            return -8;
        }

        var playerData = _host.PlayerGet(playerId);
        if (playerData == null)
        {
            return -20;
        }

        var combatId = _host.CombatStart(playerId, targetId.Value);
        if (combatId < 0)
        {
            _host.SendToClient(playerId, "Cannot start combat");
            return (int)combatId;
        }

        var action = new CombatAction
        {
            ActionType = CombatActionType.Attack,
            TargetId = targetId
        };

        return _host.CombatAction(combatId, action);
    }

    [CommandHandler("defend")]
    public int HandleDefend(string args, long playerId)
    {
        // 방어 로직 구현
        return 0;
    }

    [CommandHandler("flee")]
    public int HandleFlee(string args, long playerId)
    {
        // 도망 로직 구현
        return 0;
    }

    [EventHandler("player.login")]
    public int HandlePlayerLogin(byte[] data)
    {
        // 로그인 시 초기화 작업
        return 0;
    }

    [EventHandler("player.logout")]
    public int HandlePlayerLogout(byte[] data)
    {
        // 로그아웃 시 전투 정리
        return 0;
    }

    [TimerCallback]
    public int HandleTimer(long timerId, int callbackId)
    {
        // 전투 틱 처리
        return 0;
    }

    private long? ParseTargetId(string args)
    {
        if (long.TryParse(args.Trim(), out long id))
        {
            return id;
        }
        return null;
    }
}
```

### 10.2 SDK 인터페이스

```csharp
namespace TheProtocol.Sdk.Interfaces;

public interface IPlugin
{
    int Initialize();
    int Enable();
    int Disable();
    int Unload();
}

public interface IHostFunctions
{
    // 로깅
    void Log(int level, string message);

    // 스토리지
    byte[] StorageGet(string key);
    int StorageSet(string key, byte[] value);
    int StorageDelete(string key);

    // 이벤트
    int EmitEvent(string eventType, byte[] data);

    // 타이머
    long SetTimer(long delayMs, bool repeat, int callbackId);
    int CancelTimer(long timerId);

    // 플레이어
    PlayerData PlayerGet(long playerId);
    int PlayerUpdate(long playerId, PlayerUpdate update);

    // 인벤토리
    InventoryData InventoryGet(long playerId);
    int InventoryAddItem(long playerId, long itemId, int count);
    int InventoryRemoveItem(long playerId, long itemId, int count);

    // 전투
    long CombatStart(long attackerId, long defenderId);
    int CombatAction(long combatId, CombatAction action);

    // 통신
    int SendToClient(long playerId, string message);
    int BroadcastToRoom(long roomId, string message);
}
```
