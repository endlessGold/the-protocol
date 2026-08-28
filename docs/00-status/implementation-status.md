# 구현 현황 종합 보고서

> The Protocol 프로젝트의 현재 구현 상태를 모든 모듈별로 분석한 보고서
> 작성일: 2026-08-28

## 개요

The Protocol은 Rust 기반 크로스 플랫폼 게임 런타임 프로젝트입니다. TCP 기반 클라이언트-서버 아키텍처를 기반으로 하며, MUD 장르의 게임 로직을 구현하고 있습니다. 현재 Workspace는 13개의 crate로 구성되어 있으며, 핵심 기능의 골격은 갖추었으나 멀티플레이어 동작에 필수적인 연결 고리가 끊어져 있는 상태입니다.

## 평가 기준

| 기호 | 의미 |
|------|------|
| ✅ | 완료: 기능이 정상 동작하며 테스트 가능 |
| 🔧 | 부분 구현: 골격은 존재하나 핵심 기능 미완 |
| ❌ | 미구현: 설계 문서만 존재하거나 아예 코드 없음 |
| 🐛 | 버그 존재: 구현되었으나 알려진 문제 있음 |

---

## 모듈별 현황

### 코어 런타임 (core/)

#### core/protocol (protocol-protocol) ✅

- **구현 상태**: ✅ 완료
- **위치**: `core/protocol/src/`
- **코드 라인 수**: ~455라인 (lib.rs 7 + message.rs 291 + codec.rs 156)
- **핵심 의존성**: bytes, serde, rmp-serde, crc32fast, thiserror

**구현된 기능:**
- `MessageType` 열거형: 12개 타입 (Command, CommandResponse, Event, EventAck, Ping, Pong, Hello, HelloAck, Disconnect, Error, PluginMessage, PluginResponse)
- `Message` 구조체: 프로토콜 버전, 메시지 ID, 타입, 페이로드
- `ProtocolCodec`: 인코딩/디코딩 루트스루 테스트 통과
- 모든 게임 커맨드/응답 구조체 정의 (MoveCommand/Response, AttackCommand/Response, LookResponse, InventoryResponse, CreateCharacterCommand/Response 등)
- `Hello`/`HelloAck` 핸드셰이크 메시지
- `Direction` 열거형 + from_str()
- 클라이언트 타입 열거형: Game, MUD, Admin, Tool, Gateway, Internal

**미구현 기능:**
- 이벤트 시스템의 페이로드 직렬화/역직렬화
- WebSocket 프레임 지원

---

#### core/network (protocol-network) 🐛

- **구현 상태**: 🐛 버그 존재
- **위치**: `core/network/src/`
- **코드 라인 수**: ~170라인 (lib.rs 168 + tcp.rs 1 + udp.rs 1)
- **핵심 의존성**: tokio, bytes, tracing, thiserror, protocol-protocol, protocol-session

**구현된 기능:**
- `NetworkManager`: TCP 리스너 바인딩, 연결 수락 루프
- `SessionManager` 연동: 세션 생성, 연결 제한 확인
- TCP 연결 핸들링: Hello/HelloAck 핸드셰이크 완료
- 세션 상태 관리: Connected → Authenticated 전환
- 메시지 수신/송출 루프 (tokio::select! 기반)

**미구현/버그:**
- **네트워크 → 라우팅 연결 단절**: CommandRouter를 호출하지 않음
- `tcp.rs`: 빈 파일 (1줄 주석)
- `udp.rs`: 빈 파일 (1줄 주석)

---

#### core/session (protocol-session) 🔧

- **구현 상태**: 🔧 부분 구현
- **위치**: `core/session/src/`
- **코드 라인 수**: ~200라인 (lib.rs 129 + session.rs 71)
- **핵심 의존성**: tokio, dashmap, tracing, thiserror, protocol-protocol

**구현된 기능:**
- `SessionManager`: DashMap 기반 동시성 세션 관리
- 세션 CRUD: create_session, get, remove, get_by_address
- 연결 제한 관리: can_accept(), max_connections
- 세션 ID 자동 생성: AtomicU64 기반
- `Session`: mpsc 채널 기반 메시지 송수신
- 세션 상태 머신: Connected → Authenticating → Authenticated → InGame → Disconnected
- broadcast(), send_to(), total_connected()

**미구현 기능:**
- `Session.player_id` 설정되지 않음 (항상 None)
- 세션 타임아웃/하트비트 모니터링
- UDP/WebSocket 전송 계층

---

#### core/routing (protocol-routing) ✅

- **구현 상태**: ✅ 완료
- **위치**: `core/routing/src/`
- **코드 라인 수**: 56라인
- **핵심 의존성**: tokio, tracing, thiserror, async-trait, dashmap, protocol-protocol

**구현된 기능:**
- `CommandRouter`: DashMap 기반 커맨드 핸들러 레지스트리
- `CommandHandler` 트레이트: async handle(command, session_id)
- `route()`: 커맨드 타입별 핸들러 조회 및 디스패치
- `register()`: 핸들러 등록

**미구현 기능:**
- 미들웨어/인터셉터 체인
- 커맨드 검증/권한 필터링

---

#### core/plugin (protocol-plugin) 🔧

- **구현 상태**: 🔧 부분 구현
- **위치**: `core/plugin/src/`
- **코드 라인 수**: 177라인
- **핵심 의존성**: async-trait, serde, toml, thiserror

**구현된 기능:**
- `PluginManifest`: name, version, description, api_version, permissions, resources, dependencies
- `PluginState` 머신: Discovered → Validated → Loaded → Initialized → Enabled → Disabled
- `DefaultPluginRuntime`: 디렉토리 기반 플러그인 발견
- `PluginRuntime` 트레이트: load_all, load, enable, disable, unload

**미구현 기능:**
- WASM 플러그인 로딩/실행
- 플러그인 간 의존성 해석, 격리/샌드박싱, 핫 리로드

---

#### core/scheduler (protocol-scheduler) 🔧

- **구현 상태**: 🔧 부분 구현
- **위치**: `core/scheduler/src/`
- **코드 라인 수**: 138라인
- **핵심 의존성**: tokio, thiserror

**구현된 기능:**
- `Scheduler`: 태스크 큐 기반 스케줄러
- `schedule()`: 지연 실행 Future 등록
- `schedule_interval()`: 반복 태스크 등록
- `tick()`: 실행 시간이 된 태스크 처리
- `cancel()`: 태스크 취소

**미구현 기능:**
- 실제 Future 실행 (tick에서 스폰하지 않음)
- 태스크 상태 저장/복구, 분산 스케줄링

---

#### core/security (protocol-security) ✅

- **구현 상태**: ✅ 완료
- **위치**: `core/security/src/`
- **코드 라인 수**: 220라인
- **핵심 의존성**: dashmap, serde, thiserror

**구현된 기능:**
- `Permission`: 18개 표준 권한 + Custom
- `PluginCapabilities`: 권한 목록, 메모리 제한(64MB), 실행 시간 제한(100ms)
- `RuntimeCapabilities`: 서버/클라이언트/게이트웨이별 기능 플래그
- `CapabilityManager`: 플러그인별 권한 등록 및 확인

**미구현 기능:**
- 런타임 기능 확인 (has_runtime_capability이 항상 true)
- 레이트 리밋팅, 인증/인가 파이프라인

---

#### core/observability (protocol-observability) ✅

- **구현 상태**: ✅ 완료
- **위치**: `core/observability/src/`
- **코드 라인 수**: 24라인
- **핵심 의존성**: tracing-subscriber

**구현된 기능:**
- `init_logging()`: info 레벨 기본 로깅
- `init_logging_debug()`: debug 레벨 프리티 로깅

---

#### core/runtime (protocol-runtime) 🐛

- **구현 상태**: 🐛 버그 존재
- **위치**: `core/runtime/src/main.rs`
- **코드 라인 수**: 570라인
- **핵심 의존성**: tokio, clap, tracing, anyhow, 모든 core crate + domain + application

**구현된 기능:**
- CLI 구조 (clap): Server, Client, Gateway 서브커맨드
- 5개 내장 커맨드 핸들러: look, move, attack, inventory, create_character
- 클라이언트 모드: TCP 연결, 핸드셰이크, 대화형 루프

**버그/미구현:**
- **캐릭터 ID 하드코딩**: 모든 CommandHandler에서 character_id = 1 사용
- **CommandRouter 미사용**: 생성하지만 NetworkManager에 전달하지 않음
- Gateway 모드: 프린트만 하고 종료
- 클라이언트 세션 ID: 0 하드코딩

---

### 도메인 레이어 (domain/)

#### domain/character ✅
- **코드 라인 수**: 140라인
- CharacterClass(Warrior/Mage/Rogue/Cleric) + 기본 스탯, Stats, Character 구조체
- 데미지/치유, 경험치/레벨업 시스템
- 미구현: 스킬/마법, 장비 장착, 사망/부활

#### domain/combat ✅
- **코드 라인 수**: 103라인
- Combat, CombatState, CombatAction, 데미지 공식(str/con + 20% 분산)
- process_attack(): 데미지 → 종료 판단 → 경험치 → 이벤트
- 미구현: 방어, AI 반격, 턴 시스템

#### domain/inventory ✅
- **코드 라인 수**: 109라인
- Inventory(capacity 20), ItemStack, Item, ItemType
- add/remove/has/count 스택 아이템 관리
- 미구현: 아이템 사용, 교환, 장비 슬롯

#### domain/world ✅
- **코드 라인 수**: 205라인
- World, Room, Npc, Direction + opposite()
- 5개 방, 4개 NPC 초기화
- 미구현: 동적 맵, 문/잠금, 텔레포트

#### domain/event ✅
- **코드 라인 수**: 47라인
- DomainEvent: 9개 이벤트 타입 정의
- 미구현: 이벤트 디스패처, 로그 시스템

---

### 애플리케이션 레이어 (application/)

#### application (protocol-application) ✅
- **코드 라인 수**: ~288라인
- GameWorld 서비스: 캐릭터 생성, 방 조회, 이동, 전투, 인벤토리
- 미구현: 영속성, 동시성 제어, 전투 상태 관리

---

### 클라이언트 (clients/)

#### clients/mud (protocol-mud-client) ✅
- **코드 라인 수**: 168라인
- CLI 기반 MUD 클라이언트, 6개 커맨드 지원
- 미구현: 자동 재연결, 히스토리, 색상

---

### 플러그인 (plugins/)
- **구현 상태**: ❌ 미구현
- `plugins/` 하위 디렉토리 4개 (auction, character, combat, inventory) 존재
- 각 디렉토리에 `src/`만 있고 `.rs` 파일 없음, `Cargo.toml` 없음

### SDK (sdk/)
- **구현 상태**: ❌ 미구현
- `sdk/csharp/`: 빈 디렉토리
- `sdk/typescript/`: 빈 디렉토리

### HTTP API (api/)
- **구현 상태**: ❌ 미구현
- `api/src/`: 빈 디렉토리, `.rs` 파일 없음

### 테스트 (tests/)
- **구현 상태**: ❌ 미구현
- 완전히 빈 디렉토리

---

## 구현 진행률 요약

### 전체 완성도: 약 35%

**계산 근거:**
- 13개 workspace crate 중 완료(✅): 6개, 부분구현(🔧): 3개, 버그(🐛): 2개, 미구현(❌): 2개
- 코드 라인 수 기준: 구현됨 ~2,800라인 / 전체 예상 ~8,000라인
- 핵심 기능 기준: 프로토콜/세션/라우팅은 완료, 네트워크-라우팅 연결/멀티플레이어가 미완

### 다음 우선순위 구현 항목 (Top 10)

1. **네트워크 → 라우팅 연결**: NetworkManager에 CommandRouter 주입하여 커맨드 실제 처리
2. **캐릭터 ID 동적 할당**: Session에서 player_id 추출하여 하드코딩 제거
3. **코덱 decode() 버그 수정**: 체크섬 처리 및 버퍼 관리 재설계
4. ** plugins 뼈대 구축**: 각 plugins/ 디렉토리에 Cargo.toml + lib.rs 추가
5. **세션 하트비트/타임아웃**: 연결 유지 관리
6. **Gateway 모드 구현**: 클라이언트-서버 간 트래픽 라우팅
7. **전투 상태 관리**: 턴 기반 전투, NPC 반격
8. **테스트 작성**: 단위 테스트 및 통합 테스트
9. **api/ HTTP 레이어**: REST/WebSocket API
10. **SDK 초안**: TypeScript/C# 클라이언트 라이브러리

---

## 의존성 그래프

```
protocol-runtime (bin)
├── protocol-network
│   ├── protocol-protocol
│   └── protocol-session
│       └── protocol-protocol
├── protocol-protocol
├── protocol-session
├── protocol-plugin
├── protocol-scheduler
├── protocol-security
├── protocol-routing
│   ├── protocol-protocol
│   └── protocol-session
├── protocol-observability
├── protocol-domain
└── protocol-application
    └── protocol-domain

protocol-mud-client (bin)
├── protocol-protocol
├── protocol-network
└── protocol-observability
```

---

## 기술 부채

### 레거시 중복 파일 목록

core/ 루트에 crate 내부 파일과 동일한 6개의 loose .rs 파일 존재:

| 파일 | 중복 대상 | 차이점 |
|------|-----------|--------|
| `core/codec.rs` | `core/protocol/src/codec.rs` | 동일 (decode bug 포함) |
| `core/message.rs` | `core/protocol/src/message.rs` | 동일 |
| `core/lib.rs` | `core/session/src/lib.rs` | 세션 버전 (mpsc 채널 미포함) |
| `core/session.rs` | `core/session/src/session.rs` | 세션 버전 (mpsc 미포함, 단순 구조체) |
| `core/main.rs` | `core/runtime/src/main.rs` | proto_dir_to_domain 헬퍼 포함 |
| `core/tcp.rs` | `core/network/src/tcp.rs` | 빈 파일 |
| `core/udp.rs` | `core/network/src/udp.rs` | 빈 파일 |

### the-protocol/ 하위 폴더

- `the-protocol/`: 전체 프로젝트의 복제본 (core, domain, application, clients, api, sdk, plugins, tests, tools 등 모든 디렉토리 포함)
- `target/` 디렉토리도 포함되어 있어 디스크 공간 낭비
- 워크스페이스 Cargo.toml에 포함되지 않은 독립 구조

### 코드 품질

- 미사용 import 정리 필요 (lib.rs에서 `pub use session::Session` 등)
- `Ok(())` vs `Ok(_n)` 패턴 미통일
- `_checksum` 등 unused variable warnings
-不必要的 clone: runtime의 CommandHandler들에서 `game_world.clone()` 반복
