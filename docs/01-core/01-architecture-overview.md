# 01. 전체 시스템 아키텍처 개요

## 1. 개요

The Protocol은 Rust 기반의 교차 플랫폼 게임 런타임으로, MUD(Multi-User Dungeon) 스타일의
텍스트 기반 멀티플레이어 게임을 위한 네트워크 프레임워크이다. 이 문서는 시스템의 전체적인
아키텍처 구조, 레이어 구성, 데이터 흐름, 그리고 모듈 간 의존성 규칙을 기술한다.

## 2. 전체 시스템 아키텍처 다이어그램

```
┌─────────────────────────────────────────────────────────────────────┐
│                         The Protocol Runtime                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                    Protocol Layer (core/protocol)             │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌───────────────────┐   │  │
│  │  │ MessageType  │  │ Message Struct│  │ ProtocolCodec     │   │  │
│  │  │ (枚舉型)     │  │ (헤더+페이로드)│  │ (인코딩/디코딩)   │   │  │
│  │  └─────────────┘  └──────────────┘  └───────────────────┘   │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                              ▲                                      │
│                              │                                      │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                   Network Layer (core/network)                │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌───────────────────┐   │  │
│  │  │ TCP Listener │  │ Connection   │  │ Length-Prefix     │   │  │
│  │  │ (Tokio TCP)  │  │ Handler      │  │ Framing           │   │  │
│  │  └─────────────┘  └──────────────┘  └───────────────────┘   │  │
│  │  ┌─────────────┐  ┌──────────────┐                          │  │
│  │  │ UDP (미구현) │  │ WebSocket    │                          │  │
│  │  │              │  │ (미구현)     │                          │  │
│  │  └─────────────┘  └──────────────┘                          │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                              │                                      │
│                              ▼                                      │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                   Session Layer (core/session)                │  │
│  │  ┌─────────────────┐  ┌──────────────────────────────────┐   │  │
│  │  │ SessionManager   │  │ Session                          │   │  │
│  │  │ (DashMap 관리)   │  │ (mpsc 채널 기반 메시지 큐)       │   │  │
│  │  └─────────────────┘  └──────────────────────────────────┘   │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                              │                                      │
│                              ▼                                      │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                   Routing Layer (core/routing)                │  │
│  │  ┌─────────────────┐  ┌──────────────────────────────────┐   │  │
│  │  │ CommandRouter    │  │ CommandHandler Trait              │   │  │
│  │  │ (DashMap 핸들러) │  │ (look, move, attack 등)          │   │  │
│  │  └─────────────────┘  └──────────────────────────────────┘   │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                              │                                      │
│                              ▼                                      │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                Application Layer (application)                │  │
│  │  ┌─────────────────┐  ┌──────────────────────────────────┐   │  │
│  │  │ GameWorld        │  │ Service Logic                     │   │  │
│  │  │ (캐릭터/전투/인벤)│  │ (캐릭터 생성, 이동, 전투 처리)   │   │  │
│  │  └─────────────────┘  └──────────────────────────────────┘   │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                              │                                      │
│                              ▼                                      │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                   Domain Layer (domain)                       │  │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐        │  │
│  │  │Character │ │  World   │ │  Combat  │ │Inventory │        │  │
│  │  │ (캐릭터) │ │ (월드맵) │ │  (전투)  │ │ (인벤토리)│        │  │
│  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘        │  │
│  │  ┌─────────────────────────────────────────────────────┐     │  │
│  │  │ DomainEvent (레벨업, 공격 실행, 전투 종료 등)         │     │  │
│  │  └─────────────────────────────────────────────────────┘     │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                     │
├─────────────────────────────────────────────────────────────────────┤
│                     Cross-Cutting Concerns                          │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐             │
│  │ Security │ │Scheduler │ │ Observa- │ │ Plugin   │             │
│  │(권한 관리)│ │(스케줄러) │ │ bility   │ │ Runtime  │             │
│  │          │ │          │ │(로깅)    │ │(플러그인) │             │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘             │
└─────────────────────────────────────────────────────────────────────┘
```

## 3. 레이어 구조

### 3.1 Protocol Layer (core/protocol)

**책임**: 바이너리 프로토콜의 정의 및 직렬화/역직렬화

- `MessageType`: 12가지 메시지 타입 열거형 (Command, CommandResponse, Event, EventAck, Ping, Pong, Hello, HelloAck, Disconnect, Error, PluginMessage, PluginResponse)
- `Message`: 버전, ID, 타입, 페이로드를 포함하는 최소 단위 구조체
- `ProtocolCodec`: Length-prefix framing을 사용한 인코딩/디코딩

```rust
// 현재 구현된 메시지 포맷
struct Message {
    version: u8,          // 프로토콜 버전 (현재: 1)
    id: u64,              // 메시지 고유 ID (rand::random)
    message_type: MessageType,  // 메시지 타입
    payload: Vec<u8>,     // rmp-serde로 직렬화된 페이로드
}
```

**의존성**: 외부 의존성 없음 (standalone)

### 3.2 Network Layer (core/network)

**책임**: TCP 연결 수립, 바이트 스트림 처리, 프레임 분리

- `NetworkManager`: TCP 리스너 관리 및 연결 수락
- 연결당 `TcpStream` 분리(Reader/Writer)로 동시 읽기/쓰기 지원
- `set_nodelay(true)`로 지연 시간 최소화

```rust
struct NetworkManager {
    tcp_listener: Option<TcpListener>,
    session_manager: Arc<SessionManager>,
    codec: ProtocolCodec,
}
```

**의존성**: `protocol-protocol`, `protocol-session`

### 3.3 Session Layer (core/session)

**책임**: 클라이언트 세션 생명주기 관리, 메시지 라우팅

- `SessionManager`: DashMap 기반 동시성 안전한 세션 저장소
- `Session`: mpsc 채널 기반 양방향 메시지 큐
- `SessionState`: Connected → Authenticating → Authenticated → InGame → Disconnected

```rust
struct SessionManager {
    sessions: DashMap<u64, Session>,           // session_id → Session
    address_sessions: DashMap<SocketAddr, u64>, // 주소 → session_id
    next_id: AtomicU64,
    max_connections: usize,
}
```

**의존성**: `protocol-protocol`

### 3.4 Routing Layer (core/routing)

**책임**: 커맨드 타입별 핸들러 매핑 및 분배

- `CommandRouter`: DashMap 기반 커맨드 핸들러 레지스트리
- `CommandHandler` Trait: 비동기 핸들러 인터페이스

```rust
#[async_trait]
pub trait CommandHandler: Send + Sync {
    async fn handle(&self, command: Command, session_id: u64)
        -> Result<CommandResponse, RoutingError>;
}
```

**현재 등록된 핸들러**: look, move, attack, inventory, create_character

**의존성**: `protocol-protocol`

### 3.5 Application Layer (application)

**책임**: 게임 로직의 핵심 서비스 구현

- `GameWorld`: 캐릭터, 월드, 전투 상태 관리
- 비즈니스 규칙 검증 (이름 중복, 유효한 이동 방향 등)

```rust
struct GameWorld {
    characters: HashMap<u64, Character>,
    world: World,
    combats: HashMap<u64, Combat>,
    next_character_id: u64,
    next_combat_id: u64,
}
```

**의존성**: `domain`

### 3.6 Domain Layer (domain)

**책임**: 비즈니스 엔티티 정의 및 순수 도메인 로직

- `Character`: 캐릭터 속성, 레벨업 로직, 데미지 계산
- `World`: 방(Room), NPC, 월드 맵 구조
- `Combat`: 전투 상태, 데미지 공식, 전투 이벤트
- `Inventory`: 인벤토리 관리
- `DomainEvent`: 도메인 이벤트 (LevelUp, AttackExecuted, CombatEnded)

**의존성**: 외부 의존성 없음 (순수 Rust)

## 4. 데이터 흐름도

### 4.1 클라이언트 → 서버 (커맨드 처리)

```
[Client] ──TCP 바이트 스트림──▶ [NetworkManager]
                                    │
                                    ▼
                            [ProtocolCodec.decode_simple]
                            (4바이트 Length + 나머지 프레임 디코딩)
                                    │
                                    ▼
                            [Session.send(message)]
                            (mpsc 채널을 통해 세션의 메시지 큐에 전달)
                                    │
                                    ▼
                            [Session.incoming_rx]
                            (네트워크 메인 루프에서 recv 대기)
                                    │
                                    ▼
                            [CommandRouter.route(command)]
                            (command_type 문자열로 핸들러 매핑)
                                    │
                                    ▼
                            [CommandHandler.handle()]
                            (look, move, attack 등 비즈니스 로직 실행)
                                    │
                                    ▼
                            [GameWorld 메서드 호출]
                            (도메인 엔티티 조작)
                                    │
                                    ▼
                            [CommandResponse 생성]
                            (rmp_serde로 직렬화된 페이로드)
                                    │
                                    ▼
                            [ProtocolCodec.encode]
                            (프레임 재구성 후 TCP로 전송)
                                    │
                                    ▼
                            [Client 수신 및 출력]
```

### 4.2 서버 → 클라이언트 (이벤트 전송)

```
[GameWorld] ──DomainEvent──▶ [CommandHandler]
                                  │
                                  ▼
                          [Event 구조체 생성]
                          (event_type, source, targets 포함)
                                  │
                                  ▼
                          [SessionManager.broadcast]
                          (DashMap 순회하며 모든 세션에 전송)
                                  │
                                  ▼
                          [Session.send via mpsc]
                                  │
                                  ▼
                          [NetworkManager.write_loop]
                          (outgoing_rx에서 메시지 수신 후 TCP 전송)
```

### 4.3 핸드셰이크 흐름

```
Client                              Server
  │                                    │
  │──── Hello (protocol_version, ────▶│
  │      client_version,              │
  │      client_type,                 │
  │      auth_token)                  │
  │                                    │
  │◀─── HelloAck (session_id, ────────│
  │     protocol_version,             │
  │     server_time,                  │
  │     capabilities,                 │
  │     heartbeat_interval_ms)        │
  │                                    │
  │     [Session: Authenticating →     │
  │      Authenticated]               │
  │                                    │
  │◀═══ 이후 Command/Event 주고받음 ═══▶│
```

## 5. 모듈 간 의존성 규칙

### 5.1 의존성 방향 (상위 → 하위)

```
core/main.rs (Runtime)
    ├── core/network      (NetworkManager)
    ├── core/session       (SessionManager)
    ├── core/routing       (CommandRouter)
    ├── core/security      (CapabilityManager)
    ├── core/plugin        (PluginRuntime)
    ├── core/scheduler     (Scheduler)
    ├── core/observability (tracing)
    ├── application        (GameWorld)
    └── domain             (Character, World, Combat)
```

### 5.2 의존성 규칙

| 규칙 | 설명 |
|------|------|
| **상위 → 하위만 의존** | Domain은 어떤 모듈에도 의존하지 않음 |
| **Layer 간 직접 의존 금지** | Protocol은 Network에 의존하지 않음 (간접적 사용) |
| **Trait 기반 느슨한 결합** | `CommandHandler` Trait로 라우팅과 비즈니스 로직 분리 |
| **Arc<T> 공유** | SessionManager, GameWorld 등은 Arc로 래핑하여 여러 태스크에서 공유 |
| **mpsc 채널 통신** | Session은 mpsc 채널로 메시지를 주고받음 (블로킹 없음) |

### 5.3 현재 의존성 그래프 (Cargo)

```
protocol-runtime
    ├── protocol-network ──▶ protocol-protocol
    │                    ──▶ protocol-session ──▶ protocol-protocol
    ├── protocol-routing  ──▶ protocol-protocol
    ├── protocol-security (standalone)
    ├── protocol-plugin   (standalone)
    ├── protocol-scheduler (standalone)
    ├── protocol-observability (standalone)
    ├── application       ──▶ domain
    └── domain            (standalone)
```

## 6. "Runtime ≠ Server" 원칙

### 6.1 핵심 개념

The Protocol의 `runtime`은 단순한 서버가 아니다. 세 가지 모드를 지원하는 범용 런타임이다:

| 모드 | 설명 | Lifecycle |
|------|------|-----------|
| **Server** | 게임 월드를 호스팅하고 클라이언트를 직접 수용 | 리스너 → 핸드셰이크 → 게임 루프 |
| **Client** | 서버에 연결하여 플레이 | 연결 → 핸드셰이크 → 입력 루프 |
| **Gateway** | 클라이언트와 서버 간의 프록시/라우터 | 리스너 → 연결 에이전시 |

### 6.2 Server 모드

```
run_server()
    ├── SessionManager::new(1000)       // 최대 1000 동시 연결
    ├── NetworkManager::new(bind, sm)   // TCP 리스너 시작
    ├── DefaultPluginRuntime::new()      // 플러그인 디렉토리 스캔
    ├── CapabilityManager::new(server()) // 서버 권한 설정
    ├── CommandRouter + 핸들러 등록       // 5개 커맨드 등록
    └── NetworkManager::accept_connections() // 메인 이벤트 루프
```

**서버 모드의 역할**:
- TCP 연결 수립 및 핸드셰이크 처리
- 클라이언트 세션 생명주기 관리
- 커맨드 라우팅 및 게임 로직 실행
- 이벤트 브로드캐스트
- 플러그인 로드 및 관리

### 6.3 Client 모드

```
run_client()
    ├── TcpStream::connect(server_addr)
    ├── Hello 메시지 전송
    ├── HelloAck 수신 및 세션 ID 획득
    ├── stdin 읽기 루프 (read_line)
    └── 커맨드 → Message 변환 → TCP 전송 → 응답 수신
```

**클라이언트 모드의 역할**:
- 서버 연결 및 프로토콜 핸드셰이크
- 사용자 입력을 프로토콜 메시지로 변환
- 서버 응답을 텍스트로 디스플레이

### 6.4 Gateway 모드 (미구현)

```
run_gateway()
    └── "Gateway mode - routing traffic between clients and servers."
```

**목표 역할**:
- 클라이언트 연결 수용 (TCP/WebSocket)
- 백엔드 서버로 프록시
- 부하 분산 및 로드 밸런싱
- 인증 게이트웨이

### 6.5 왜 Runtime인가?

1. **모드 독립성**: 같은 바이너리로 서버/클라이언트/게이트웨이 실행 가능
2. **확장성**: 각 모드는 독립적으로 배포 및 확장 가능
3. **责任制**: 런타임은 "실행 환경"을 제공하고, 비즈니스 로직은 레이어 분리
4. **능력 기반(Capability)**: `RuntimeCapabilities`로 각 모드별 허용 기능 제어

```rust
impl RuntimeCapabilities {
    pub fn server() -> Self { /* tcp_listener, udp_listener, http_server, ... */ }
    pub fn client() -> Self { /* tcp_client, udp_client, http_client, ... */ }
    pub fn gateway() -> Self { /* tcp_listener, tcp_client, udp_*, ... */ }
}
```

## 7. 확장 가능 영역

### 7.1 미구현 주요 기능

| 영역 | 현재 상태 | 필요 작업 |
|------|-----------|-----------|
| **UDP 전송** | `core/network/src/udp.rs` - 빈 파일 | 설계 및 구현 필요 |
| **HTTP 서버** | 미구현 | Axum 기반 REST API 서버 |
| **WebSocket** | `TransportType::WebSocket` 정의만 | ws:// 전송 레이어 구현 |
| **설정 시스템** | 하드코딩 (기본값 사용) | TOML 기반 설정 로드 |
| **데이터베이스** | `RuntimeCapabilities.database: false` | 영속화 레이어 |
| **캐시** | 미구현 | Redis 또는 인메모리 캐시 |
| **모니터링** | `tracing` 로깅만 | Prometheus 메트릭 |
| **인증** | Hello에 auth_token 포함 (검증 미구현) | JWT/OAuth 인증 |

### 7.2 확장 포인트

1. **플러그인 시스템**: `PluginRuntime` Trait 기반으로 새 기능 추가
2. **커맨드 라우터**: `CommandHandler` Trait 구현으로 새 커맨드 등록
3. **전송 계층**: `TransportType` 열거형 확장으로 새 전송 프로토콜 추가
4. **이벤트 시스템**: `DomainEvent` 기반 구독/발행 패턴 확장
5. **보안 레이어**: `Permission` 열거형 및 `PluginCapabilities`로 세밀한 권한 제어

### 7.3 성능 최적화 가능 영역

- `ProtocolCodec.decode`에서 불필요한 `clone()` 제거 (현재 `peek` 용도)
- `Session`의 `Arc<Mutex<mpsc::Receiver>>` → `tokio::sync::Mutex` 사용 최적화
- `SessionManager::broadcast`에서 전체 순회 대상 필터링 개선
- `GameWorld`의 `RwLock<GameWorld>` → 읽기 빈도 기반 분리 잠금 고려

## 8. Workspace 구성

```
The Protocol/
├── Cargo.toml              (workspace root)
├── core/
│   ├── protocol/           (프로토콜 정의)
│   ├── network/            (TCP/UDP/WS 네트워크)
│   ├── session/            (세션 관리)
│   ├── routing/            (커맨드 라우팅)
│   ├── security/           (권한 관리)
│   ├── plugin/             (플러그인 런타임)
│   ├── scheduler/          (스케줄러)
│   ├── observability/      (로깅/모니터링)
│   └── runtime/            (메인 바이너리 - clap CLI)
├── application/            (비즈니스 서비스)
├── domain/                 (도메인 엔티티)
├── clients/
│   └── mud/                (MUD 클라이언트)
├── the-protocol/           (기존/레거시 코드)
├── api/                    (REST API - 미구현)
├── plugins/                (플러그인 디렉토리)
├── sdk/                    (SDK - 미구현)
├── tools/                  (개발 도구)
└── docs/                   (문서)
```

## 9. 요약

The Protocol은 **5개의 핵심 레이어**와 **4개의 교차 관심사 모듈**로 구성된 정교한 런타임 아키텍처이다. 각 레이어는 명확한 책임을 가지며, Trait 기반의 느슨한 결합으로 확장성을 보장한다. "Runtime ≠ Server" 원칙을 통해 하나의 바이너리로 서버/클라이언트/게이트웨이 세 가지 모드를 지원하며, 향후 UDP, WebSocket, HTTP 등 다양한 전송 계층으로의 확장이 설계 단계부터 고려되어 있다.
