# 03. 네트워크 레이어 상세 설계

## 1. 개요

네트워크 레이어는 The Protocol의 클라이언트-서버 통신 기반을 담당한다.
현재 TCP 전송만 구현되어 있으며, UDP, HTTP/Axum, WebSocket은 설계만 완료되어 있다.
이 문서는 각 전송 프로토콜의 구현 상세, 연결 관리, 프레이밍, 리소스 관리 등을 기술한다.

## 2. TCP 리스너 구현 상세

### 2.1 구조체 정의 (`core/network/src/lib.rs`)

```rust
pub struct NetworkManager {
    tcp_listener: Option<TcpListener>,       // Tokio TCP 리스너
    session_manager: Arc<SessionManager>,     // 세션 관리자 (공유)
    codec: ProtocolCodec,                     // 프로토콜 코덱
}
```

### 2.2 초기화 및 바인딩

```rust
pub async fn new(
    bind_address: &str,                        // 예: "127.0.0.1:7770"
    session_manager: Arc<SessionManager>,
) -> Result<Self, NetworkError> {
    let tcp_listener = TcpListener::bind(bind_address).await?;
    tracing::info!("TCP listening on {}", bind_address);
    // → "TCP listening on 127.0.0.1:7770"
}
```

**바인딩 동작**:
- `TcpListener::bind()`로 주소 바인딩
- 기본값: `127.0.0.1:7770` (CLI 인자로 변경 가능)
- 바인딩 실패 시 `NetworkError::Io` 반환 (포트 충돌 등)

### 2.3 연결 수락 루프

```rust
pub async fn accept_connections(&self) -> Result<(), NetworkError> {
    let listener = self.tcp_listener.as_ref().ok_or(NetworkError::Closed)?;

    loop {
        // 1. 새 연결 수락
        let (socket, addr) = listener.accept().await?;

        // 2. 연결 가능 여부 확인
        if !self.session_manager.can_accept() {
            tracing::warn!("Connection limit reached, rejecting {}", addr);
            drop(socket);  // 즉시 연결 종료
            continue;
        }

        // 3. TCP_NODELAY 활성화 (지연 시간 최소화)
        socket.set_nodelay(true)?;

        // 4. 핸들러를 별도 태스크로 스폰
        let session_manager = self.session_manager.clone();
        let codec = self.codec.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::handle_connection(
                socket, addr, codec, session_manager
            ).await {
                tracing::error!("Connection error from {}: {}", addr, e);
            }
        });
    }
}
```

**핵심 동작**:
1. `accept()`로 새 TCP 연결 수립 대기
2. `can_accept()`로 동시 연결 제한 확인 (현재: 1000개)
3. `set_nodelay(true)`로 Nagle 알고리즘 비활성화
4. 연결 처리를 `tokio::spawn`으로 비동기 태스크 분리
5. 핸들러 실패 시 로그 기록

## 3. 연결 풀 관리

### 3.1 현재 구현

현재 연결 풀은 `SessionManager`의 `DashMap`으로 관리된다:

```rust
pub struct SessionManager {
    sessions: DashMap<u64, Session>,           // session_id → Session
    address_sessions: DashMap<SocketAddr, u64>, // 클라이언트 주소 → session_id
    next_id: AtomicU64,                        // 세션 ID 시퀀스
    max_connections: usize,                     // 최대 연결 수
}
```

### 3.2 연결 관리 동작

| 작업 | 메서드 | 설명 |
|------|--------|------|
| 연결 수락 | `create_session()` | 새 세션 생성 및 DashMap에 저장 |
| 연결 확인 | `can_accept()` | `sessions.len() < max_connections` |
| 세션 조회 | `get(session_id)` | DashMap에서 세션 반환 (Option\<Session\>) |
| 주소 조회 | `get_by_address(addr)` | IP:Port로 세션 ID 조회 |
| 연결 제거 | `remove(session_id)` | 세션 및 주소 매핑 제거 |
| 전체 전송 | `broadcast(message)` | 모든 세션에 메시지 전송 |

### 3.3 연결 한도

```rust
// 서버 초기화 시
let session_manager = Arc::new(SessionManager::new(1000)); // 최대 1000 연결

// 연결 수락 시
if !self.session_manager.can_accept() {
    tracing::warn!("Connection limit reached, rejecting {}", addr);
    drop(socket); // TCP RST 전송
    continue;
}
```

**한도 초과 시 동작**:
- 새 연결 즉시 거부 (`drop(socket)` → TCP RST)
- 클라이언트에 에러 메시지 미전송 (연결 자체를 거부)
- 로그에 경고 기록

### 3.4 개선 검토 사항

| 항목 | 현재 | 개선 방안 |
|------|------|-----------|
| 동시 연결 제한 | 1000개 고정 | 설정 가능하도록 변경 |
| IP 기반 제한 | 없음 | IP당 최대 연결 수 제한 |
| 연결 대기열 | Tokio 기본 | backlog 파라미터 설정 |
| 리소스 한도 | 미구현 | 메모리/CPU 사용량 기반 제한 |

## 4. TCP 프레이밍 (Length-prefix Framing)

### 4.1 프레이밍 원리

```
TCP 바이트 스트림:
[0][0][0][18][1][0][0][0][0][0][0][55][32][...][CRC32][0][0][0][14][1]...

       │                              │
       ▼                              ▼
  프레임 1 (Length=18)           프레임 2 (Length=14)
  [Version][ID][Type][Payload]  [Version][ID][Type][Payload]
```

### 4.2 수신 프레이밍 과정

```rust
// 1. Length 헤더 읽기 (4바이트)
let mut len_buf = [0u8; 4];
reader.read_exact(&mut len_buf).await?;
let total_len = u32::from_be_bytes(len_buf) as usize;

// 2. 나머지 프레임 읽기 (total_len - 4 바이트)
let mut frame = vec![0u8; total_len - 4];
reader.read_exact(&mut frame).await?;

// 3. 전체 프레임 조립
let mut full_frame = BytesMut::with_capacity(4 + total_len);
full_frame.put_slice(&len_buf);
full_frame.put_slice(&frame);

// 4. 디코딩
let message = codec.decode_simple(&mut buf)?;
```

### 4.3 디코딩 상세 (`core/protocol/src/codec.rs`)

```rust
pub fn decode_simple(buf: &mut BytesMut) -> Result<Option<Message>, CodecError> {
    // 최소 프레임 크기 확인 (Length 4 + Version 1 + ID 8 + Type 1 + Checksum 4 = 18)
    if buf.len() < 4 {
        return Ok(None); // 데이터 부족
    }

    // Length 읽기 (peek)
    let total_len = { let mut peek = buf.clone(); peek.get_u32() as usize };

    if buf.len() < total_len {
        return Ok(None); // 프레임 불완전
    }

    // 필드 파싱
    buf.get_u32();                                    // Length 소비
    let version = buf.get_u8();                       // Version
    let id = buf.get_u64();                           // MessageID
    let message_type_byte = buf.get_u8();             // Type

    let message_type = MessageType::from_u8(message_type_byte)
        .ok_or(CodecError::InvalidMessageType(message_type_byte))?;

    // Payload 읽기
    let payload_len = total_len - 14 - 4;             // Length - 헤더(14) - 체크섬(4)
    let mut payload = vec![0u8; payload_len];
    buf.copy_to_slice(&mut payload);

    let _checksum = buf.get_u32();                    // Checksum (검증 미구현)

    Ok(Some(Message { version, id, message_type, payload }))
}
```

### 4.4 인코딩 상세

```rust
pub fn encode(&self, message: &Message) -> Result<BytesMut, CodecError> {
    let payload = rmp_serde::to_vec(&message.payload)?;
    let checksum = crc32fast::hash(&payload);         // CRC32 계산

    let total_len = 14 + payload.len() + 4;
    let mut buf = BytesMut::with_capacity(total_len);

    buf.put_u32(total_len as u32);                     // 4 bytes: Length
    buf.put_u8(message.version);                       // 1 byte:  Version
    buf.put_u64(message.id);                           // 8 bytes: MessageID
    buf.put_u8(message.message_type as u8);            // 1 byte:  Type
    buf.put_slice(&payload);                           // N bytes: Payload
    buf.put_u32(checksum);                             // 4 bytes: Checksum

    Ok(buf)
}
```

## 5. 현재 핸드셰이크 과정

### 5.1 서버 측 (`core/network/src/lib.rs:79-120`)

```
Step 1: TCP 연결 수락
    listener.accept() → (socket, addr)

Step 2: 세션 생성
    session_manager.create_session(addr, Tcp) → session_id
    상태: Connected

Step 3: Hello 수신
    reader.read_exact(4 bytes) → Length
    reader.read_exact(Length-4 bytes) → 나머지 프레임
    codec.decode_simple() → Hello { protocol_version, client_version, client_type, auth_token }

Step 4: HelloAck 전송
    Message::hello_ack(session_id, capabilities)
    codec.encode() → 바이트
    writer.write_all()

Step 5: 세션 상태 변경
    session.set_state(Authenticated)
    상태: Connected → Authenticating → Authenticated
```

### 5.2 클라이언트 측 (`core/main.rs:112-140`)

```
Step 1: TCP 연결
    TcpStream::connect(server_addr) → stream

Step 2: Hello 전송
    Message::hello(ClientType::MUD, None)
    codec.encode() → 바이트
    writer.write_all()

Step 3: HelloAck 수신
    reader.read_exact(4 bytes) → Length
    reader.read_exact(Length-4 bytes) → 나머지
    codec.decode_simple() → HelloAck { session_id, ... }

Step 4: 세션 ID 확인
    println!("Connected! Session: {}", hello_ack.session_id)
```

### 5.3 핸드셰이크 상태 다이어그램

```
[Client]                          [Server]
    │                                │
    │  TCP 연결 수립                  │
    │  ─────────────────────────────▶│
    │                                │
    │                    [session_id = next_id()]
    │                    [state = Connected]
    │                                │
    │  Hello (protocol_version,     │
    │         client_type, ...)     │
    │  ─────────────────────────────▶│
    │                                │
    │                    [Hello 디코딩]
    │                    [버전 검증 (미구현)]
    │                    [인증 검증 (미구현)]
    │                                │
    │  HelloAck (session_id,        │
    │            capabilities, ...) │
    │  ◀─────────────────────────────│
    │                                │
    │                    [state = Authenticated]
    │                                │
    │  ◀════════此后 Command/Event ══▶│
```

## 6. 미구현: UDP 전송 (설계)

### 6.1 설계 목표

- **용도**: 실시간 위치 업데이트, 상태 동기화 등 지연 시간 민감한 데이터
- **특성**: 연결 기반 아님 (connectionless), 순서 보장 불가, 패킷 손실 가능
- **한계**: 단일 세션에서만 사용 (Handshake는 TCP)

### 6.2 프레임 구조

```
UDP Frame Layout:
┌──────────┬──────────┬──────────┬──────────┬────────────┐
│ Sequence │ Version  │MessageID │  Type    │  Payload   │
│ 4bytes   │  1byte   │  8bytes  │  1byte   │  N bytes   │
│ u32BE    │  u8      │  u64BE   │  u8      │  Vec<u8>   │
└──────────┴──────────┴──────────┴──────────┴────────────┘

Note: Length 불필요 (UDP는 메시지 단위 수신)
Note: Checksum 불필요 (UDP 자체 checksum 사용)
```

### 6.3 시퀀스 관리

```rust
struct UdpSession {
    local_sequence: AtomicU32,      // 송신 시퀀스
    remote_sequence: AtomicU32,     // 수신 시퀀스
    received_bits: AtomicU64,       // 비트마스킹 수신 확인
    pending_acks: DashMap<u32, Instant>,  // ACK 대기 메시지
}

impl UdpSession {
    fn next_sequence(&self) -> u32 {
        self.local_sequence.fetch_add(1, Ordering::Relaxed)
    }

    fn on_received(&self, seq: u32) -> bool {
        let remote = self.remote_sequence.load(Ordering::Relaxed);
        if seq > remote {
            self.remote_sequence.store(seq, Ordering::Relaxed);
            true
        } else {
            false // 오래된 시퀀스 (무시)
        }
    }
}
```

### 6.4 불량 복구 (NACK)

```
Client                              Server
  │                                    │
  │── UDP: Seq=10 ──────────────────▶│
  │── UDP: Seq=11 (손실)              │
  │── UDP: Seq=12 ──────────────────▶│
  │                                    │
  │                                    │ [Seq=11 미수신 감지]
  │                                    │
  │◀── NACK: Seq=11 ─────────────────│
  │                                    │
  │── UDP: Seq=11 (재전송) ─────────▶│
```

## 7. 미구현: HTTP/Axum 서버 (설계)

### 7.1 설계 목표

- **용도**: REST API, 관리 패널, 상태 조회
- **프레임워크**: Axum (Tokio 기반)
- **포트**: 7771 (HTTP), 7772 (HTTPS)

### 7.2 API 엔드포인트 설계

```
GET  /api/status              → 서버 상태 (온라인 플레이어 수, uptime)
GET  /api/players             → 플레이어 목록
GET  /api/players/{id}        → 플레이어 상세
GET  /api/world/rooms         → 방 목록
GET  /api/world/rooms/{id}    → 방 상세
POST /api/auth/login          → 인증 (JWT 발급)
POST /api/admin/broadcast     → 전체 브로드캐스트 (관리자)
GET  /api/metrics             → Prometheus 메트릭
```

### 7.3 구조 설계

```rust
use axum::{Router, routing::{get, post}, Json};

pub fn create_router(session_manager: Arc<SessionManager>, game_world: Arc<RwLock<GameWorld>>) -> Router {
    Router::new()
        .route("/api/status", get(server_status))
        .route("/api/players", get(list_players))
        .route("/api/players/:id", get(get_player))
        .route("/api/world/rooms", get(list_rooms))
        .route("/api/world/rooms/:id", get(get_room))
        .route("/api/auth/login", post(login))
        .route("/api/admin/broadcast", post(admin_broadcast))
        .route("/api/metrics", get(metrics))
        .with_state(AppState { session_manager, game_world })
}
```

## 8. 미구현: WebSocket 전송 (설계)

### 8.1 설계 목표

- **용도**: 브라우저 클라이언트 지원, 프록시/게이트웨이용
- **프레임워크**: `tokio-tungstenite` 또는 Axum WebSocket
- **포트**: TCP 리스너와 동일 포트 (Upgrade)

### 8.2 WebSocket ↔ Protocol 매핑

```
WebSocket Frame              Protocol Frame
─────────────────           ──────────────
Text Frame    ─────────────▶  ASCII 커맨드 (MUD 스퀴)
Binary Frame  ─────────────▶  Length-prefix 프레임
Ping Frame    ─────────────▶  Protocol Ping
Pong Frame    ─────────────▶  Protocol Pong
Close Frame   ─────────────▶  Disconnect
```

### 8.3 구조 설계

```rust
pub struct WebSocketTransport {
    ws_stream: SplitSink<WebSocket, Message>,
    codec: ProtocolCodec,
}

impl WebSocketTransport {
    pub async fn send(&mut self, message: &Message) -> Result<(), TransportError> {
        let encoded = self.codec.encode(message)?;
        self.ws_stream.send(Message::Binary(encoded.to_vec())).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Option<Message>, TransportError> {
        match self.ws_stream.next().await {
            Some(Ok(Message::Binary(data))) => {
                let mut buf = BytesMut::from(&data[..]);
                Ok(self.codec.decode_simple(&mut buf)?)
            }
            Some(Ok(Message::Text(text))) => {
                // MUD 스타일 텍스트 커맨드 처리
                let message = parse_text_command(&text)?;
                Ok(Some(message))
            }
            _ => Ok(None),
        }
    }
}
```

## 9. 연결당 리소스 관리

### 9.1 연결당 할당 리소스

| 리소스 | 현재 | 한도 |
|--------|------|------|
| `mpsc::channel` | 256 메시지 버퍼 | 채널당 256개 |
| `TcpStream` | Reader/Writer 분리 | OS 파일 디스크립터 |
| `Session` 구조체 | 약 200 bytes | 고정 |
| `BytesMut` 프레임 버퍼 | 동적 할당 | 메시지당 |

### 9.2 리소스 정리

```rust
// 연결 종료 시
session_manager.remove(session_id);
// → DashMap에서 Session 제거
// → address_sessions에서 매핑 제료
// → mpsc 채널 닫힘 (자동)
// → TcpStream 닫힘 (drop)
```

### 9.3 메모리 사용량 추정

```
세션당 메모리:
  Session 구조체:      ~200 bytes
  mpsc 채널 버퍼:      256 × ~64 bytes = ~16 KB
  TcpStream:           OS 관리 (약 8 KB)
  프레임 버퍼:         최대 ~64 KB
  ─────────────────────────────────
  합계:               약 88 KB/세션

1000 세션 시:          약 88 MB
```

### 9.4 리소스 모니터링

```rust
// 현재 구현
pub fn count(&self) -> usize { self.sessions.len() }
pub fn total_connected(&self) -> u64 {
    self.next_id.load(Ordering::Relaxed) - 1
}

// 개선 필요 항목:
// - 세션당 메모리 사용량
// - 네트워크 대역폭
// - CPU 사용량
// - 파일 디스크립터 수
```

## 10. Keepalive/Heartbeat 설계

### 10.1 현재 상태

```rust
// HelloAck에서 클라이언트에 전달
HelloAck {
    heartbeat_interval_ms: 30000,  // 30초 간격
    ...
}
```

**문제**: 서버와 클라이언트 모두 하트비트를 처리하는 로직이 미구현

### 10.2 설계안

#### 클라이언트 측

```rust
async fn heartbeat_loop(writer: &mut WriteHalf, interval: Duration) {
    let mut timer = tokio::time::interval(interval);
    loop {
        timer.tick().await;
        let ping = Message::ping();
        writer.write_all(&codec.encode(&ping)).await?;
    }
}
```

#### 서버 측

```rust
async fn session_timeout_check(session_manager: Arc<SessionManager>) {
    let mut timer = tokio::time::interval(Duration::from_secs(10));
    loop {
        timer.tick().await;
        let now = Instant::now();
        for entry in session_manager.sessions.iter() {
            let session = entry.value();
            let elapsed = now.duration_since(session.last_activity);
            if elapsed > Duration::from_secs(90) { // 3 × heartbeat
                tracing::info!("Session {} timed out", session.id);
                session_manager.remove(session.id);
            }
        }
    }
}
```

### 10.3 하트비트 상태 다이어그램

```
[Client]                    [Server]
    │                          │
    │  Ping ──────────────────▶│
    │                          │  last_activity 갱신
    │  ◀──────────────────── Pong│
    │                          │
    │  ... (30초 후) ...        │
    │                          │
    │  Ping ──────────────────▶│
    │                          │
    │  ... (응답 없음) ...      │
    │                          │
    │  ... (30초 후) ...        │
    │                          │
    │  Ping ──────────────────▶│
    │                          │  3회 미수신 감지
    │                          │  세션 제거
    │                          │
    │  Connection reset        │
```

## 11. Rate Limiting 설계

### 11.1 설계 목표

- 세션별 커맨드 처리 속도 제한
- 서버 부하 보호
- 악성 클라이언트 차단

### 11.2 설계안

```rust
struct RateLimiter {
    limits: DashMap<String, TokenBucket>,  // session_id → 버킷
}

struct TokenBucket {
    max_tokens: u32,           // 최대 토큰 수
    tokens: f64,               // 현재 토큰
    refill_rate: f64,          // 초당 리필율
    last_refill: Instant,      // 마지막 리필 시각
}

impl TokenBucket {
    fn try_consume(&mut self, tokens: u32) -> bool {
        self.refill();
        if self.tokens >= tokens as f64 {
            self.tokens -= tokens as f64;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate)
            .min(self.max_tokens as f64);
        self.last_refill = now;
    }
}
```

### 11.3 적용 기준

| 커맨드 유형 | 토큰 소모 | 제한 |
|-------------|-----------|------|
| look | 1 | 초당 10회 |
| move | 2 | 초당 5회 |
| attack | 3 | 초당 3회 |
| inventory | 1 | 초당 10회 |
| create_character | 5 | 분당 3회 |

## 12. 네트워크 모니터링

### 12.1 현재 구현

```rust
// 로깅 (tracing)
tracing::info!("TCP listening on {}", bind_address);
tracing::warn!("Connection limit reached, rejecting {}", addr);
tracing::error!("Connection error from {}: {}", addr, e);
tracing::info!("Session {} handshake complete", session_id);
tracing::info!("Session {} disconnected: {}", session_id, e);
```

### 12.2 개선 권장 모니터링 항목

| 항목 | 수집 방법 | 설명 |
|------|-----------|------|
| 동시 연결 수 | `session_manager.count()` | 실시간 연결 수 |
| 총 연결 수 | `session_manager.total_connected()` | 누적 연결 수 |
| 연결/해제율 | 이벤트 카운터 | 초당 연결/해제 수 |
| 메시지 처리량 | 카운터 | 초당 메시지 수 |
| 대역폭 | 바이트 카운터 | 초당 송수신 바이트 |
| 레이턴시 | 타임스탬프 비교 | 커맨드 응답 시간 |
| 에러율 | 에러 카운터 | 초당 에러 수 |

### 12.3 Prometheus 메트릭 설계

```
# 메트릭 이름 규칙
protocol_network_connections_total          (게이지)
protocol_network_messages_sent_total        (카운터)
protocol_network_messages_received_total    (카운터)
protocol_network_bytes_sent_total           (카운터)
protocol_network_bytes_received_total       (카운터)
protocol_network_errors_total               (카운터)
protocol_network_session_duration_seconds   (히스토그램)
protocol_network_command_duration_seconds   (히스토그램)
```

## 13. 요약

| 구성 요소 | 현재 상태 | 다음 단계 |
|-----------|-----------|-----------|
| TCP 리스너 | ✅ 구현 완료 | backlog 설정, SSL/TLS |
| 연결 풀 | ✅ 구현 완료 | IP 기반 제한, 모니터링 |
| TCP 프레이밍 | ✅ 구현 완료 | 체크섬 검증 |
| 핸드셰이크 | ✅ 구현 완료 | 인증 검증, 에러 핸들링 |
| UDP 전송 | ❌ 미구현 | 시퀀스 관리, NACK |
| HTTP 서버 | ❌ 미구현 | Axum 기반 REST API |
| WebSocket | ❌ 미구현 | 텍스트/바이너리 변환 |
| 하트비트 | ❌ 미구현 | Ping/Pong 자동화 |
| Rate Limiting | ❌ 미구현 | 토큰 버킷 알고리즘 |
| 모니터링 | ⚠️ 로깅만 | Prometheus 메트릭 |
