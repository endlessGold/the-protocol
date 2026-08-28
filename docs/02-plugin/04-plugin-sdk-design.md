# 플러그인 SDK 설계

> The Protocol 외부 개발자를 위한 TypeScript/C# SDK 아키텍처 및 개발 워크플로우

## 1. 개요

The Protocol 플러그인 SDK는 외부 개발자가 플러그인을 쉽게 개발할 수 있도록 도구와 인터페이스를 제공합니다. TypeScript와 C# 두 가지 언어를 지원하며, 각 SDK는 WASM 컴파일 파이프라인을 통해 런타임이 실행 가능한 형식으로 변환합니다.

## 2. TypeScript SDK 아키텍처

### 2.1 npm 패키지 구조

```
@the-protocol/sdk/
├── package.json
├── tsconfig.json
├── src/
│   ├── index.ts              # 메인 엔트리포인트
│   ├── types/
│   │   ├── index.ts          # 타입 정의
│   │   ├── player.ts         # Player 관련 타입
│   │   ├── inventory.ts      # Inventory 관련 타입
│   │   ├── combat.ts         # Combat 관련 타입
│   │   ├── events.ts         # Event 관련 타입
│   │   └── storage.ts        # Storage 관련 타입
│   ├── host/
│   │   ├── index.ts          # Host Functions 엔트리
│   │   ├── logging.ts        # log 함수
│   │   ├── storage.ts        # storage_* 함수
│   │   ├── events.ts         # emit_event 함수
│   │   ├── timers.ts         # set_timer, cancel_timer 함수
│   │   ├── player.ts         # player_* 함수
│   │   ├── inventory.ts      # inventory_* 함수
│   │   ├── combat.ts         # combat_* 함수
│   │   └── communication.ts  # send_to_client, broadcast_to_room 함수
│   ├── buffer/
│   │   ├── manager.ts        # Buffer 관리
│   │   └── codec.ts          # MessagePack 인코딩/디코딩
│   ├── lifecycle/
│   │   ├── manager.ts        # 생명주기 관리
│   │   └── registry.ts       # 핸들러 레지스트리
│   └── utils/
│       ├── serialization.ts  # 직렬화 유틸리티
│       └── validation.ts     # 입력 검증
├── dist/                     # 빌드 결과물
├── examples/
│   ├── hello-world/
│   ├── combat-system/
│   └── inventory-manager/
└── README.md
```

### 2.2 TypeScript → WASM 컴파일 파이프라인

#### 옵션 1: AssemblyScript (권장)

```
TypeScript Source → AssemblyScript → WASM
```

**장점:**
- TypeScript 문법 최대한 지원
- 빠른 컴파일 속도
- 작은 WASM 파일 크기
- AssemblyScript 커뮤니티 활발

**단점:**
- 완전한 TypeScript 지원 아님 (제한적)
- 일부 라이브러리 호환성 문제

```
┌─────────────────────────────────────────────────────┐
│           AssemblyScript 컴파일 파이프라인              │
│                                                      │
│  ① TypeScript 소스                                    │
│       ↓                                              │
│  ② AssemblyScript 컴파일러 (asc)                      │
│       ↓                                              │
│  ③ .wasm 파일 생성                                    │
│       ↓                                              │
│  ④ 최적화 (wasm-opt)                                 │
│       ↓                                              │
│  ⑤ .wasm 파일 (최종)                                  │
└─────────────────────────────────────────────────────┘
```

**빌드 스크립트:**

```json
{
  "scripts": {
    "build": "asc src/index.ts --outFile dist/plugin.wasm --optimize",
    "build:debug": "asc src/index.ts --outFile dist/plugin.wasm --debug",
    "test": "node --experimental-wasm test/runner.js",
    "package": "npm run build && tar -czf plugin.tar.gz dist/plugin.wasm plugin.toml"
  },
  "devDependencies": {
    "assemblyscript": "^0.27.0",
    "@as-pect/core": "^8.0.0"
  }
}
```

#### 옵션 2: wasm-bindgen + wasm-pack

```
TypeScript Source → wasm-bindgen → Rust → WASM
```

**장점:**
- Rust 생태계 통합
- 강한 타입 안전성
- 풍부한 기능 지원

**단점:**
- Rust 필요 (학습 곡선)
- 컴파일 시간 김

#### 옵션 3: TinyGo

```
TypeScript → Go → TinyGo → WASM
```

**장점:**
- Go 언어 사용 가능
- 작은 바이너리 크기

**단점:**
- TypeScript 지원 없음
- Go 커뮤니티에서의 WASM 지원 제한적

### 2.3 SDK 함수 레퍼런스

#### registerCommand

```typescript
/**
 * 명령어 핸들러를 등록합니다.
 * @param command - 명령어 이름
 * @param handler - 명령어 처리 함수
 */
export function registerCommand(
  command: string,
  handler: (args: string, playerId: number) => number
): void;

// 사용 예시
registerCommand("heal", (args, playerId) => {
  const amount = parseInt(args) || 10;
  const player = player_get(playerId);
  if (!player) return -20;

  const newHp = Math.min(player.hp + amount, player.max_hp);
  player_update(playerId, { hp: newHp });

  send_to_client(playerId, `You healed for ${amount} HP.`);
  emit_event("player.healed", { playerId, amount });
  return 0;
});
```

#### registerEventHandler

```typescript
/**
 * 이벤트 핸들러를 등록합니다.
 * @param eventType - 이벤트 타입
 * @param handler - 이벤트 처리 함수
 */
export function registerEventHandler(
  eventType: string,
  handler: (data: ArrayBuffer) => number
): void;

// 사용 예시
registerEventHandler("player.login", (data) => {
  const player = deserialize<PlayerData>(data);
  log(2, `Player logged in: ${player.name}`);
  return 0;
});
```

#### emitEvent

```typescript
/**
 * 이벤트를 발행합니다.
 * @param eventType - 이벤트 타입
 * @param data - 이벤트 데이터
 */
export function emitEvent(eventType: string, data: any): number;

// 사용 예시
emitEvent("item.picked", { itemId: 123, playerId: 456 });
```

#### getStorage / setStorage

```typescript
/**
 * 스토리지에서 값을 가져옵니다.
 * @param key - 키
 * @returns 값 또는 null
 */
export function getStorage<T>(key: string): T | null;

/**
 * 스토리지에 값을 저장합니다.
 * @param key - 키
 * @param value - 값
 */
export function setStorage<T>(key: string, value: T): number;

// 사용 예시
interface PluginConfig {
  enabled: boolean;
  maxLevel: number;
}

const config = getStorage<PluginConfig>("config");
if (config) {
  log(2, `Config loaded: maxLevel=${config.maxLevel}`);
}

setStorage("config", { enabled: true, maxLevel: 100 });
```

### 2.4 타입 안전성

```typescript
// types/player.ts
export interface PlayerData {
  id: number;
  name: string;
  level: number;
  hp: number;
  maxHp: number;
  mp: number;
  maxMp: number;
  position: Position;
  stats: Stats;
  statusEffects: StatusEffect[];
}

export interface Position {
  roomId: number;
  x: number;
  y: number;
  z: number;
}

export interface Stats {
  strength: number;
  dexterity: number;
  intelligence: number;
  constitution: number;
  wisdom: number;
  charisma: number;
}

export interface StatusEffect {
  effectType: string;
  duration: number;
  magnitude: number;
}

// types/inventory.ts
export interface InventoryData {
  items: InventorySlot[];
  gold: number;
  weight: number;
  maxWeight: number;
}

export interface InventorySlot {
  slotIndex: number;
  itemId: number;
  itemName: string;
  count: number;
  properties: Record<string, string>;
}

// types/combat.ts
export interface CombatAction {
  actionType: "Attack" | "Defend" | "UseItem" | "UseSkill" | "Flee";
  targetId?: number;
  itemId?: number;
  skillId?: string;
}

export interface CombatResult {
  success: boolean;
  damage?: number;
  healing?: number;
  statusEffects: StatusEffect[];
  message: string;
  combatEnded: boolean;
  winner?: number;
}

// types/events.ts
export interface PluginEvent {
  source: string;
  eventType: string;
  data: ArrayBuffer;
  timestamp: string;
}
```

### 2.5 개발 워크플로우

```
┌─────────────────────────────────────────────────────┐
│              개발 워크플로우                            │
│                                                      │
│  ① 프로젝트 생성                                      │
│     npx @the-protocol/create-plugin my-plugin        │
│                                                      │
│  ② 개발                                              │
│     npm run dev (핫 리로드)                           │
│                                                      │
│  ③ 테스트                                             │
│     npm test (로컬 WASM 런타임에서 테스트)              │
│                                                      │
│  ④ 빌드                                              │
│     npm run build                                    │
│                                                      │
│  ⑤ 배포                                              │
│     npm run publish (레지스트리에 배포)                 │
└─────────────────────────────────────────────────────┘
```

**프로젝트 생성:**

```bash
npx @the-protocol/create-plugin my-plugin
cd my-plugin
npm install
npm run dev
```

**생성된 프로젝트 구조:**

```
my-plugin/
├── plugin.toml
├── package.json
├── tsconfig.json
├── src/
│   └── index.ts
├── test/
│   └── plugin.test.ts
└── README.md
```

## 3. C# SDK 아키텍처

### 3.1 NuGet 패키지 구조

```
TheProtocol.Sdk/
├── TheProtocol.Sdk.csproj
├── Attributes/
│   ├── PluginAttribute.cs
│   ├── CommandHandlerAttribute.cs
│   ├── EventHandlerAttribute.cs
│   ├── TimerCallbackAttribute.cs
│   ├── RequiredPermissionsAttribute.cs
│   ├── MemoryLimitAttribute.cs
│   ├── ExecutionLimitAttribute.cs
│   └── DependencyAttribute.cs
├── Interfaces/
│   ├── IPlugin.cs
│   ├── IHostFunctions.cs
│   └── ILogger.cs
├── Types/
│   ├── PlayerData.cs
│   ├── InventoryData.cs
│   ├── CombatAction.cs
│   └── Events.cs
├── Host/
│   ├── HostFunctions.cs
│   └── BufferManager.cs
└── Serialization/
    └── MessagePackSerializer.cs
```

### 3.2 C# → WASM 파이프라인

#### 옵션 1: Blazor WASM (권장)

```
C# Source → .NET WASM → WASM
```

**장점:**
- .NET 생태계 완전 지원
- 강한 타입 안전성
- 풍부한 라이브러리

**단점:**
-较大的 WASM 크기
- .NET 런타임 오버헤드

**빌드 스크립트:**

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <RuntimeIdentifier>browser-wasm</RuntimeIdentifier>
    <OutputType>Library</OutputType>
  </PropertyGroup>

  <ItemGroup>
    <PackageReference Include="TheProtocol.Sdk" Version="1.0.0" />
  </ItemGroup>
</Project>
```

```bash
dotnet build -r browser-wasm
```

#### 옵션 2: .NET WASM AOT

```
C# Source → .NET AOT → WASM
```

**장점:**
- 네이티브 코드 수준의 성능
- 작은 바이너리 크기

**단점:**
- .NET AOT 제약사항
- 일부 .NET 기능 제한

### 3.3 SDK 함수 레퍼런스

```csharp
// SDK를 사용한 플러그인 작성 예시
[Plugin(Name = "my-plugin", Version = "1.0.0")]
[RequiredPermissions("player.read", "player.write")]
[MemoryLimit("32MB")]
[Dependency("base-plugin", ">=1.0.0")]
public class MyPlugin : IPlugin
{
    private readonly IHostFunctions _host;
    private readonly ILogger _logger;

    public MyPlugin(IHostFunctions host, ILogger logger)
    {
        _host = host;
        _logger = logger;
    }

    public int Initialize()
    {
        _logger.Info("Plugin initialized");
        return 0;
    }

    public int Enable()
    {
        _logger.Info("Plugin enabled");
        return 0;
    }

    public int Disable()
    {
        _logger.Info("Plugin disabled");
        return 0;
    }

    public int Unload()
    {
        _logger.Info("Plugin unloaded");
        return 0;
    }

    [CommandHandler("greet")]
    public int HandleGreet(string args, long playerId)
    {
        var player = _host.PlayerGet(playerId);
        if (player == null) return -20;

        _host.SendToClient(playerId, $"Hello, {player.Name}!");
        return 0;
    }

    [EventHandler("player.login")]
    public int HandlePlayerLogin(byte[] data)
    {
        var player = MessagePackSerializer.Deserialize<PlayerData>(data);
        _logger.Info($"Player logged in: {player.Name}");
        return 0;
    }

    [TimerCallback]
    public int HandleTimer(long timerId, int callbackId)
    {
        _logger.Info($"Timer tick: {timerId}");
        return 0;
    }
}
```

## 4. 공통 SDK 인터페이스

### 4.1 플러그인 인터페이스

```typescript
// TypeScript
interface Plugin {
  init(): number;
  enable(): number;
  disable(): number;
  unload(): number;
  handle_command(command: string, args: string, player_id: number): number;
  handle_event(event_type: string, data: ArrayBuffer): number;
  handle_timer(timer_id: number, callback_id: number): number;
}
```

```csharp
// C#
public interface IPlugin
{
    int Initialize();
    int Enable();
    int Disable();
    int Unload();
}
```

### 4.2 Host Functions 인터페이스

```typescript
// TypeScript
interface HostFunctions {
  // 로깅
  log(level: number, message: string): void;

  // 스토리지
  storage_get(key: string): ArrayBuffer | null;
  storage_set(key: string, value: ArrayBuffer): number;
  storage_delete(key: string): number;

  // 이벤트
  emit_event(event_type: string, data: ArrayBuffer): number;

  // 타이머
  set_timer(delay_ms: number, repeat: boolean, callback_id: number): number;
  cancel_timer(timer_id: number): number;

  // 플레이어
  player_get(player_id: number): PlayerData | null;
  player_update(player_id: number, update: PlayerUpdate): number;

  // 인벤토리
  inventory_get(player_id: number): InventoryData | null;
  inventory_add_item(player_id: number, item_id: number, count: number): number;
  inventory_remove_item(player_id: number, item_id: number, count: number): number;

  // 전투
  combat_start(attacker_id: number, defender_id: number): number;
  combat_action(combat_id: number, action: CombatAction): number;

  // 통신
  send_to_client(player_id: number, message: string): number;
  broadcast_to_room(room_id: number, message: string): number;
}
```

```csharp
// C#
public interface IHostFunctions
{
    void Log(int level, string message);
    byte[] StorageGet(string key);
    int StorageSet(string key, byte[] value);
    int StorageDelete(string key);
    int EmitEvent(string eventType, byte[] data);
    long SetTimer(long delayMs, bool repeat, int callbackId);
    int CancelTimer(long timerId);
    PlayerData PlayerGet(long playerId);
    int PlayerUpdate(long playerId, PlayerUpdate update);
    InventoryData InventoryGet(long playerId);
    int InventoryAddItem(long playerId, long itemId, int count);
    int InventoryRemoveItem(long playerId, long itemId, int count);
    long CombatStart(long attackerId, long defenderId);
    int CombatAction(long combatId, CombatAction action);
    int SendToClient(long playerId, string message);
    int BroadcastToRoom(long roomId, string message);
}
```

### 4.3 직렬화 프로토콜

양 SDK는 동일한 직렬화 형식(MessagePack)을 사용합니다:

```typescript
// TypeScript (MessagePack)
import { encode, decode } from "@msgpack/msgpack";

const encoded = encode({ name: "test", value: 123 });
const decoded = decode(encoded);
```

```csharp
// C# (MessagePack)
using MessagePack;

var encoded = MessagePackSerializer.Serialize(new { Name = "test", Value = 123 });
var decoded = MessagePackSerializer.Deserialize<dynamic>(encoded);
```

## 5. 플러그인 테스트 프레임워크

### 5.1 테스트 구조

```
test/
├── unit/
│   ├── commands.test.ts
│   ├── events.test.ts
│   └── timers.test.ts
├── integration/
│   ├── combat.test.ts
│   └── inventory.test.ts
├── fixtures/
│   ├── players.json
│   └── items.json
└── runner.ts
```

### 5.2 테스트 헬퍼

```typescript
// test/runner.ts
import { PluginTestRunner, MockHost } from "@the-protocol/test-utils";

async function runTests() {
  const mockHost = new MockHost();
  const runner = new PluginTestRunner(mockHost);

  // 플러그인 로드
  await runner.loadPlugin("./dist/plugin.wasm");

  // 명령어 테스트
  await runner.testCommand("heal", "10", 1001, (result) => {
    assert.equal(result, 0);
    assert.equal(mockHost.getPlayer(1001).hp, 60);
  });

  // 이벤트 테스트
  await runner.testEvent("player.login", { id: 1001 }, (result) => {
    assert.equal(result, 0);
  });

  // 타이머 테스트
  await runner.testTimer(1, 100, (result) => {
    assert.equal(result, 0);
  });

  console.log("All tests passed!");
}

runTests();
```

### 5.3 Mock Host 구현

```typescript
// MockHost
export class MockHost implements HostFunctions {
  private players: Map<number, PlayerData> = new Map();
  private storage: Map<string, ArrayBuffer> = new Map();
  private events: Array<{ type: string; data: ArrayBuffer }> = [];
  private timers: Array<{ id: number; delay: number; callback: number }> = [];

  log(level: number, message: string): void {
    console.log(`[${level}] ${message}`);
  }

  storage_get(key: string): ArrayBuffer | null {
    return this.storage.get(key) || null;
  }

  storage_set(key: string, value: ArrayBuffer): number {
    this.storage.set(key, value);
    return 0;
  }

  player_get(player_id: number): PlayerData | null {
    return this.players.get(player_id) || null;
  }

  emit_event(event_type: string, data: ArrayBuffer): number {
    this.events.push({ type: event_type, data });
    return 0;
  }

  // ... 기타 함수 구현
}
```

### 5.4 테스트 명령줄 도구

```bash
# 단일 테스트 실행
npm test -- --grep "combat system"

# 통합 테스트 실행
npm run test:integration

# 커버리지 리포트
npm run test:coverage
```

## 6. 플러그인 배포 (레지스트리)

### 6.1 레지스트리 아키텍처

```
┌─────────────────────────────────────────────────────┐
│              Plugin Registry                         │
│                                                      │
│  ┌───────────────────────────────────────────────┐  │
│  │              Web UI                           │  │
│  │  - 플러그인 검색                               │  │
│  │  - 플러그인 상세 정보                          │  │
│  │  - 다운로드 카운트                             │  │
│  │  - 리뷰/평점                                  │  │
│  └───────────────────────────────────────────────┘  │
│                                                      │
│  ┌───────────────────────────────────────────────┐  │
│  │              API Server                       │  │
│  │  - REST API                                   │  │
│  │  - GraphQL                                    │  │
│  │  - 인증/권한                                  │  │
│  └───────────────────────────────────────────────┘  │
│                                                      │
│  ┌───────────────────────────────────────────────┐  │
│  │              Storage                          │  │
│  │  - 플러그인 WASM 파일                          │  │
│  │  - 매니페스트                                 │  │
│  │  - 메타데이터                                 │  │
│  │  - 버전 이력                                  │  │
│  └───────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

### 6.2 레지스트리 API

```
POST   /api/plugins                 # 플러그인 업로드
GET    /api/plugins                 # 플러그인 목록
GET    /api/plugins/:name           # 플러그인 상세
GET    /api/plugins/:name/versions  # 버전 목록
GET    /api/plugins/:name/download  # 플러그인 다운로드
DELETE /api/plugins/:name           # 플러그인 삭제
```

### 6.3 업로드 프로세스

```
┌─────────────────────────────────────────────────────┐
│              업로드 프로세스                           │
│                                                      │
│  ① SDK에서 npm run publish 실행                      │
│                                                      │
│  ② 빌드 (TypeScript → WASM)                         │
│                                                      │
│  ③ 매니페스트 검증                                   │
│     - 필수 필드 확인                                 │
│     - API 버전 검증                                  │
│     - 의존성 확인                                    │
│                                                      │
│  ④ WASM 모듈 검증                                    │
│     - 컴파일 확인                                   │
│     - Export 함수 확인                               │
│     - Host Function 의존성 확인                      │
│                                                      │
│  ⑤ 레지스트리에 저장                                 │
│                                                      │
│  ⑥ CDN 배포                                         │
└─────────────────────────────────────────────────────┘
```

### 6.4 버전 관리 전략

- Semantic Versioning (semver) 준수
- MAJOR 버전 변경 시 API 호환성 검증
- 최대 10개 버전 유지 (이전 버전 자동 정리)
- 릴리스 노트 작성 필수

### 6.5 플러그인 설치/업데이트

```bash
# CLI를 통한 설치
the-protocol plugin install combat-system@1.0.0

# 전체 플러그인 업데이트
the-protocol plugin update

# 특정 플러그인 업데이트
the-protocol plugin update combat-system
```

## 7. 문서화 전략

### 7.1 문서 구조

```
docs/
├── getting-started/
│   ├── installation.md         # 설치 가이드
│   ├── quickstart.md           # 빠른 시작
│   └── project-structure.md    # 프로젝트 구조
├── guides/
│   ├── creating-plugins.md     # 플러그인 생성 가이드
│   ├── host-functions.md       # Host Function 사용법
│   ├── events.md               # 이벤트 시스템
│   ├── storage.md              # 스토리지 사용법
│   ├── combat.md               # 전투 시스템 연동
│   └── testing.md              # 테스트 가이드
├── api-reference/
│   ├── host-functions/         # Host Function API 레퍼런스
│   ├── plugin-exports/         # Plugin Export API 레퍼런스
│   ├── types/                  # 타입 정의
│   └── errors.md               # 에러 코드 참조
├── examples/
│   ├── hello-world/
│   ├── combat-system/
│   ├── inventory-manager/
│   └── custom-command/
└── contributing/
    ├── sdk-development.md      # SDK 기여 가이드
    └── plugin-guidelines.md    # 플러그인 개발 가이드라인
```

### 7.2 자동화된 문서 생성

```typescript
// SDK에서 JSDoc 주석을 기반으로 문서 자동 생성
/**
 * 플레이어 데이터를 조회합니다.
 * @param player_id - 플레이어 ID
 * @returns 플레이어 데이터 또는 null
 * @example
 * ```typescript
 * const player = player_get(1001);
 * if (player) {
 *   console.log(player.name);
 * }
 * ```
 */
export function player_get(player_id: number): PlayerData | null;
```

### 7.3 인터랙티브 예시

- CodeSandbox 통합: 브라우저에서 바로 테스트 가능
- 스텝별 튜토리얼: 초보자向け 가이드
- 비디오 가이드: 복잡한 개념 시각화

### 7.4 문서 업데이트 전략

- SDK 버전 업데이트 시 문서 자동 업데이트
- 커뮤니티 기여를 위한 PR 프로세스
- 정기적인 문서 리뷰 (월 1회)
- 사용자 피드백 반영

## 8. 개발자 경험 (DX) 최적화

### 8.1 CLI 도구

```bash
# 플러그인 생성
the-protocol create my-plugin

# 플러그인 개발 서버
the-protocol dev

# 플러그인 빌드
the-protocol build

# 플러그인 테스트
the-protocol test

# 플러그인 배포
the-protocol publish
```

### 8.2 IDE 지원

- VS Code 확장 프로그램
  - 자동 완성
  - 에러 하이라이팅
  - Host Function 힌트
  - 디버깅 지원

### 8.3 린팅/포맷팅

```json
{
  "scripts": {
    "lint": "eslint src/ --ext .ts",
    "format": "prettier --write src/"
  },
  "devDependencies": {
    "@typescript-eslint/eslint-plugin": "^6.0.0",
    "eslint": "^8.0.0",
    "prettier": "^3.0.0"
  }
}
```

### 8.4 성능 프로파일러

```typescript
// 플러그인 성능 측정
import { profile } from "@the-protocol/sdk";

const result = profile("combat-computation", () => {
  // 복잡한 계산
  return computeCombatResult(attacker, defender);
});

console.log(`Execution time: ${result.duration}ms`);
```
