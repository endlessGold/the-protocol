# 04. 세션 관리 상세 설계

## 1. 개요

세션 관리 레이어는 클라이언트 연결의 생명주기, 상태 관리, 메시지 큐잉을 담당한다.
The Protocol의 세션은 mpsc 채널 기반으로 설계되어 네트워크 I/O와 비즈니스 로직을 분리한다.
이 문서는 세션의 전체 생명주기, 구조체 상세, 저장소 전략, 분산 확장 등을 기술한다.

## 2. 세션 생명주기

### 2.1 상태 정의 (`core/session/src/lib.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connected,        // TCP 연결 수립됨 (핸드셰이크 미완료)
    Authenticating,   // Hello 수신 후 인증 처리 중
    Authenticated,    // 핸드셰이크 완료 (HelloAck 전송)
    InGame,           // 캐릭터 선택/생성 완료 (게임 플레이 중)
    Disconnected,     // 연결 종료
}
```

### 2.2 상태 전이 다이어그램

```
                    [TCP 연결 수립]
                         │
                         ▼
                    ┌─────────────┐
                    │  Connected  │ ← 세션 생성 시 초기 상태
                    └──────┬──────┘
                           │
                    Hello 수신
                           │
                           ▼
                    ┌──────────────────┐
                    │ Authenticating   │
                    └──────┬───────────┘
                           │
                    HelloAck 전송
                           │
                           ▼
                    ┌──────────────────┐
                    │  Authenticated   │ ← 기본 대기 상태
                    └──────┬───────────┘
                           │
                    set_player() 호출
                    (캐릭터 선택/생성)
                           │
                           ▼
                    ┌──────────────────┐
                    │     InGame       │ ← 게임 플레이 중
                    └──────┬───────────┘
                           │
                    연결 끊김 / Disconnect
                           │
                           ▼
                    ┌──────────────────┐
                    │  Disconnected    │ ← 최종 상태
                    └──────────────────┘
```

### 2.3 상태 전이 코드

```rust
// 서버 측 상태 전이 (core/network/src/lib.rs)
// 1. 세션 생성
let session_id = session_manager.create_session(addr, TransportType::Tcp)?;
// → state = Connected

// 2. Hello 수신 후
// → (별도 상태 변경 없음 - Authenticating은 향후 구현)

// 3. HelloAck 전송 후
session.set_state(SessionState::Authenticated);
// → state = Authenticated

// 4. 캐릭터 선택 시 (향후 구현)
session.set_player(player_id);
// → state = InGame
```

## 3. 세션 구조체 필드 상세

### 3.1 Session 구조체 (`core/session/src/session.rs`)

```rust
#[derive(Clone)]
pub struct Session {
    pub id: u64,                                    // 세션 고유 ID (AtomicU64에서 할당)
    pub player_id: Option<u64>,                     // 연결된 플레이어 ID (InGame 시)
    pub address: SocketAddr,                        // 클라이언트 IP:Port
    pub transport: TransportType,                   // 전송 타입 (Tcp/Udp/WebSocket)
    pub state: SessionState,                        // 현재 세션 상태
    pub connected_at: Instant,                      // 연결 시각
    pub last_activity: Instant,                     // 마지막 활동 시각
    outgoing_tx: mpsc::Sender<Message>,             // 메시지 송신 채널
    incoming_rx: Arc<Mutex<mpsc::Receiver<Message>>>, // 메시지 수신 채널
}
```

### 3.2 필드 상세 설명

| 필드 | 타입 | 설명 | 생성 시 값 |
|------|------|------|-----------|
| `id` | `u64` | 세션 고유 식별자 | `AtomicU64`에서 `fetch_add(1)` |
| `player_id` | `Option<u64>` | 인 게임 플레이어 ID | `None` |
| `address` | `SocketAddr` | 클라이언트 소켓 주소 | 수락 시 획득한 주소 |
| `transport` | `TransportType` | TCP/UDP/WebSocket | `TransportType::Tcp` |
| `state` | `SessionState` | 현재 상태 | `SessionState::Connected` |
| `connected_at` | `Instant` | 연결 시작 시각 | `Instant::now()` |
| `last_activity` | `Instant` | 마지막 활동 시각 | `Instant::now()` |
| `outgoing_tx` | `mpsc::Sender` | 메시지 송신 채널 | `mpsc::channel(256).0` |
| `incoming_rx` | `Arc<Mutex<mpsc::Receiver>>` | 메시지 수신 채널 | `mpsc::channel(256).1` |

### 3.3 TransportType

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportType {
    Tcp,          // TCP 전송
    Udp,          // UDP 전송 (미구현)
    WebSocket,    // WebSocket 전송 (미구현)
}
```

### 3.4 메서드 상세

```rust
impl Session {
    // 새 세션 생성
    pub fn new(
        id: u64,
        address: SocketAddr,
        transport: TransportType,
        outgoing_tx: mpsc::Sender<Message>,
        incoming_rx: mpsc::Receiver<Message>,
    ) -> Self { ... }

    // 메시지 전송 (논블로킹)
    pub fn send(&self, message: Message) -> Result<(), SessionError> {
        self.outgoing_tx
            .try_send(message)  // 논블로킹 전송
            .map_err(|_| super::SessionError::Closed)
    }

    // 메시지 수신 (블로킹)
    pub async fn recv(&self) -> Option<Message> {
        let mut rx = self.incoming_rx.lock().await;
        rx.recv().await
    }

    // 상태 변경
    pub fn set_state(&mut self, state: SessionState) {
        self.state = state;
    }

    // 플레이어 연결 (InGame 상태로 전이)
    pub fn set_player(&mut self, player_id: u64) {
        self.player_id = Some(player_id);
        self.state = SessionState::InGame;
    }

    // 활동 시간 갱신
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }
}
```

## 4. 세션 생성/제거 플로우

### 4.1 세션 생성 (`SessionManager::create_session`)

```
[NetworkManager: accept_connections]
    │
    │ listener.accept() → (socket, addr)
    │
    ▼
[SessionManager::can_accept() = true?]
    │
    ├─ No → drop(socket) → 경고 로그 → continue
    │
    ├─ Yes ↓
    │
    ▼
[session_id = next_id()]
    │ AtomicU64: fetch_add(1, Relaxed)
    │
    ▼
[mpsc::channel(256) → (tx, rx)]
    │ 채널 버퍼: 256 메시지
    │
    ▼
[Session::new(session_id, addr, Tcp, tx, rx)]
    │
    ▼
[DashMap::insert(session_id, session)]
    │ sessions.insert(session_id, session)
    │
    ▼
[DashMap::insert(addr, session_id)]
    │ address_sessions.insert(addr, session_id)
    │
    ▼
[로그: "Session {} created from {} ({:?})"]
    │
    ▼
[return session_id]
```

### 4.2 세션 제거 (`SessionManager::remove`)

```
[세션 제거 트리거]
    │
    ├─ TCP 연결 끊김 (reader.read_exact 실패)
    ├─ Disconnect 메시지 수신
    ├─ 타임아웃
    │
    ▼
[SessionManager::remove(session_id)]
    │
    ▼
[DashMap::remove(session_id)]
    │ sessions.remove(session_id) → Option<(Key, Session)>
    │
    ├─ None → return None (이미 제거됨)
    │
    ├─ Some(session) ↓
    │
    ▼
[DashMap::remove(session.address)]
    │ address_sessions.remove(addr)
    │
    ▼
[세션 자원 정리]
    │ outgoing_tx 드롭 → 채널 닫힘
    │ incoming_rx 드롭 → 수신 종료
    │ TcpStream 드롭 → OS 정리
    │
    ▼
[로그: "Session {} removed"]
    │
    ▼
[return Some(session)]
```

### 4.3 네트워크 핸들러 내 세션 관리 (`core/network/src/lib.rs`)

```rust
async fn handle_connection(...) -> Result<(), NetworkError> {
    // 세션 생성
    let session_id = session_manager.create_session(addr, TransportType::Tcp)?;

    // ... 핸드셰이크 및 메인 루프 ...

    // 메인 루프 종료 후 세션 제거
    session_manager.remove(session_id);
    Ok(())
}
```

**메인 루프 종료 조건**:
1. 클라이언트가 연결을 끊음 (`reader.read_exact` 실패)
2. 서버가 Disconnect 전송
3. 네트워크 에러 발생

## 5. 세션 ↔ 플레이어 매핑

### 5.1 현재 구현

```rust
// Session 구조체
pub player_id: Option<u64>,  // 플레이어 ID (미설정 시 None)

// 플레이어 연결
pub fn set_player(&mut self, player_id: u64) {
    self.player_id = Some(player_id);
    self.state = SessionState::InGame;
}
```

### 5.2 매핑 관계

```
SessionManager
    │
    ├─ sessions: DashMap<u64, Session>
    │      │
    │      ├─ 1 → Session { id: 1, player_id: None, state: Authenticated }
    │      ├─ 2 → Session { id: 2, player_id: Some(42), state: InGame }
    │      └─ 3 → Session { id: 3, player_id: Some(77), state: InGame }
    │
    └─ address_sessions: DashMap<SocketAddr, u64>
           │
           ├─ 127.0.0.1:50001 → 1
           ├─ 192.168.1.10:8080 → 2
           └─ 10.0.0.5:12345 → 3
```

### 5.3 플레이어 세션 조회

```rust
// 세션 ID로 플레이어 조회
if let Some(session) = session_manager.get(session_id) {
    if let Some(player_id) = session.player_id {
        // InGame 상태 - 게임 로직 처리
    }
}

// 주소로 세션 조회
if let Some(session_id) = session_manager.get_by_address(&addr) {
    // 해당 주소의 세션 ID 획득
}
```

### 5.4 향후 개선: 양방향 매핑

```rust
// 개선된 SessionManager
pub struct SessionManager {
    sessions: DashMap<u64, Session>,               // session_id → Session
    address_sessions: DashMap<SocketAddr, u64>,     // address → session_id
    player_sessions: DashMap<u64, u64>,             // player_id → session_id (추가)
}

impl SessionManager {
    pub fn get_session_by_player(&self, player_id: u64) -> Option<Session> {
        self.player_sessions.get(&player_id)
            .and_then(|entry| self.sessions.get(entry.value()).map(|s| s.clone()))
    }
}
```

## 6. 세션 타임아웃/하트비트

### 6.1 현재 상태

- `HelloAck`에 `heartbeat_interval_ms: 30000` 포함 (전달만 함)
- 서버/클라이언트 모두 하트비트 처리 로직 미구현
- `last_activity` 필드 존재 (갱신 메서드 `touch()` 있음)

### 6.2 설계안

#### 타임아웃 설정

```rust
pub struct SessionConfig {
    pub heartbeat_interval_ms: u64,     // 30000 (30초)
    pub heartbeat_timeout_multiplier: f64, // 2.5
    pub max_session_duration_secs: u64,  // 3600 (1시간)
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval_ms: 30000,
            heartbeat_timeout_multiplier: 2.5,
            max_session_duration_secs: 3600,
        }
    }
}
```

#### 하트비트 처리기

```rust
pub async fn heartbeat_handler(
    session_manager: Arc<SessionManager>,
    config: SessionConfig,
) {
    let check_interval = Duration::from_millis(config.heartbeat_interval_ms / 2);
    let timeout = Duration::from_millis(
        (config.heartbeat_interval_ms as f64 * config.heartbeat_timeout_multiplier) as u64
    );
    let max_duration = Duration::from_secs(config.max_session_duration_secs);

    let mut timer = tokio::time::interval(check_interval);

    loop {
        timer.tick().await;
        let now = Instant::now();

        for entry in session_manager.sessions.iter() {
            let session = entry.value();
            let session_id = *entry.key();

            // 1. 하트비트 타임아웃 확인
            let idle_time = now.duration_since(session.last_activity);
            if idle_time > timeout {
                tracing::info!(
                    "Session {} timed out (idle: {:?})",
                    session_id, idle_time
                );
                drop(entry);
                session_manager.remove(session_id);
                continue;
            }

            // 2. 최대 세션 시간 확인
            let session_duration = now.duration_since(session.connected_at);
            if session_duration > max_duration {
                tracing::info!(
                    "Session {} exceeded max duration ({:?})",
                    session_id, session_duration
                );
                drop(entry);
                session_manager.remove(session_id);
                continue;
            }
        }
    }
}
```

### 6.3 하트비트 메시지 교환

```
[Server]                              [Client]
    │                                    │
    │  HelloAck                          │
    │  { heartbeat_interval_ms: 30000 }  │
    │  ─────────────────────────────────▶│
    │                                    │
    │              [30초마다 Ping 전송 시작]
    │                                    │
    │  ◀── Ping ─────────────────────────│
    │                                    │
    │  Pong ────────────────────────────▶│
    │  [last_activity 갱신]              │
    │                                    │
    │              ... (반복) ...         │
```

## 7. 세션 직렬화 (분산 확장용)

### 7.1 목적

- 멀티 서버 간 세션 동기화
- 세션 영속화 (서버 재시작 시 복원)
- 부하 분산 시 세션 이동

### 7.2 직렬화 구조체

```rust
#[derive(Serialize, Deserialize)]
struct SerializableSession {
    id: u64,
    player_id: Option<u64>,
    address: String,               // SocketAddr을 문자열로
    transport: TransportType,
    state: SessionState,
    connected_at: u64,             // Unix timestamp (밀리초)
    last_activity: u64,            // Unix timestamp (밀리초)
    // Note: mpsc 채널은 직렬화 불가 - 재생성 필요
}

impl From<&Session> for SerializableSession {
    fn from(s: &Session) -> Self {
        Self {
            id: s.id,
            player_id: s.player_id,
            address: s.address.to_string(),
            transport: s.transport,
            state: s.state,
            connected_at: s.connected_at
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            last_activity: s.last_activity
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }
}
```

### 7.3 직렬화 제약 조건

| 필드 | 직렬화 가능 | 설명 |
|------|-------------|------|
| `id` | ✅ | u64 |
| `player_id` | ✅ | Option\<u64\> |
| `address` | ✅ | SocketAddr → String |
| `transport` | ✅ | Serialize/Deserialize 구현 |
| `state` | ✅ | Serialize/Deserialize 구현 |
| `connected_at` | ✅ | Instant → u64 변환 필요 |
| `last_activity` | ✅ | Instant → u64 변환 필요 |
| `outgoing_tx` | ❌ | mpsc::Sender는 직렬화 불가 |
| `incoming_rx` | ❌ | mpsc::Receiver는 직렬화 불가 |

**해결 방안**: 직렬화 시 채널 필드 제외, 복원 시 새 채널 생성

## 8. 세션 저장소 선택

### 8.1 DashMap vs Custom 비교

| 항목 | DashMap (현재) | Custom (RwLock<HashMap>) |
|------|----------------|--------------------------|
| 동시성 | 수준 높은 수준 (분리 잠금) | 읽기/쓰기 전체 잠금 |
| 성능 | 높은 동시 읽기 | 읽기 빈도 높을 때 유리 |
| 메모리 | 약간 더 높음 (버킷 구조) | 약간 낮음 |
| 구현 복잡도 | 낮음 (크레이트 사용) | 중간 |
| 기능 | 기본 CRUD + Iterator | 완전한 제어 |

### 8.2 DashMap 사용 이유

```rust
// 현재 구현
use dashmap::DashMap;

pub struct SessionManager {
    sessions: DashMap<u64, Session>,           // 높은 동시성
    address_sessions: DashMap<SocketAddr, u64>, // 주소 기반 조회
    next_id: AtomicU64,
    max_connections: usize,
}
```

**DashMap 장점**:
1. **분리 잠금 (Sharded Lock)**: 세션 A 쓰기가 세션 B 읽기를 차단하지 않음
2. **단일 API**: `insert`, `get`, `remove` 등 직관적
3. **Iterator**: `iter()`로 전체 순회 가능
4. **타입 안전**: `Ref` 래퍼로 동시성 보장

### 8.3 개선 검토: 분리 잠금 전략

```rust
// 개선안: 읽기/쓰기 분리
pub struct SessionManager {
    sessions: DashMap<u64, Session>,             // 세션 데이터
    state_index: DashMap<u64, SessionState>,     // 상태 인덱스 (빠른 필터링)
    player_index: DashMap<u64, u64>,             // player_id → session_id
    address_index: DashMap<SocketAddr, u64>,     // address → session_id
    next_id: AtomicU64,
    max_connections: usize,
}
```

## 9. 멀티 서버 세션 공유

### 9.1 아키텍처 설계

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   Server 1   │     │   Server 2   │     │   Server 3   │
│ SessionMgr   │     │ SessionMgr   │     │ SessionMgr   │
└──────┬───────┘     └──────┬───────┘     └──────┬───────┘
       │                    │                    │
       └────────────────────┼────────────────────┘
                            │
                    ┌───────▼───────┐
                    │  Redis/的消息总线  │
                    │  (세션 공유)    │
                    └───────────────┘
```

### 9.2 세션 공유 프로토콜

```
[서버 1: 세션 생성]
    │
    ├─ 로컬 세션 생성
    ├─ Redis에 세션 정보 저장
    │   SET session:{id} {json}
    │   SET player:{player_id} {session_id}
    │
    ▼
[세션 이벤트 발행]
    │
    ├─ Redis Pub/Sub: PUBLISH session:events {created, session_id, server_id}
    │
    ▼
[서버 2, 3에서 이벤트 수신]
    │
    ├─ 로컬 캐시 업데이트 (세션 메타데이터)
    ├─ 실제 메시지 라우팅은 오리진 서버로 포워딩
```

### 9.3 세션 라우팅 규칙

```
[클라이언트 → Gateway → 서버]

1. 클라이언트 연결
2. Gateway에서 세션 ID 확인
3. 세션 ID로 오리진 서버 조회
   - Redis: GET session:{id}:origin → server_id
4. 해당 서버로 메시지 포워딩
5. 서버에서 응답을 Gateway로 전달
6. Gateway에서 클라이언트로 응답
```

### 9.4 Redis 키 구조

```
session:{session_id}              → SerializableSession (JSON)
session:{session_id}:origin       → server_id (문자열)
session:{session_id}:player       → player_id (문자열)
player:{player_id}:session        → session_id (문자열)
server:{server_id}:sessions       → Set<session_id>
```

## 10. 세션 관련 에러 처리

### 10.1 SessionError

```rust
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("Session not found: {0}")]
    NotFound(u64),           // 세션 ID로 조회 실패

    #[error("Session closed")]
    Closed,                  // 세션 채널 닫힘 (연결 종료)
}
```

### 10.2 에러 시나리오

| 상황 | 에러 | 처리 |
|------|------|------|
| 세션 ID로 조회 실패 | `NotFound(id)` | 에러 로그, 연결 종료 |
| 세션 채널 가득 참 | `Closed` (try_send 실패) | 세션 제거 |
| 세션 중복 제거 | `None` 반환 | 무시 |
| 연결 한도 초과 | `SessionError::Closed` | 새 연결 거부 |

## 11. 세션 성능 최적화

### 11.1 현재 병목

| 위치 | 문제 | 심각도 |
|------|------|--------|
| `Session::recv()` | `Arc<Mutex<mpsc::Receiver>>` 뮤텍스 잠금 | 중간 |
| `SessionManager::broadcast()` | 전체 세션 순회 | 높음 |
| `session_manager.get()` | DashMap 참조 카운팅 | 낮음 |

### 11.2 개선 방안

```rust
// 개선 1: recv()에서 뮤텍스 제거
// 현재: Arc<Mutex<mpsc::Receiver<Message>>>
// 개선: mpsc::Receiver를 Session 생성 시 분리

// 개선 2: broadcast 최적화
// 현재: 전체 순회
// 개선: 상태별 인덱스 (InGame 세션만 순회)

// 개선 3: 세션 풀 재사용
// 현재: 매 세션마다 새 mpsc::channel 할당
// 개선: 사전 할당된 채널 풀에서 재사용
```

## 12. 요약

| 구성 요소 | 현재 상태 | 설명 |
|-----------|-----------|------|
| 세션 생명주기 | ✅ 5상태 정의 | Connected → Disconnected |
| 세션 구조체 | ✅ 구현 완료 | mpsc 기반 양방향 채널 |
| 세션 생성/제거 | ✅ 구현 완료 | DashMap + AtomicU64 |
| 플레이어 매핑 | ⚠️ 기본만 | session → player 일방향 |
| 타임아웃/하트비트 | ⚠️ 필드만 | 처리 로직 미구현 |
| 직렬화 | ❌ 미구현 | 분산 확장용 |
| 저장소 | ✅ DashMap | 성능 적합 |
| 멀티 서버 공유 | ❌ 미구현 | Redis 기반 설계 |
| 성능 최적화 | ⚠️ 기본 | broadcast 개선 필요 |
