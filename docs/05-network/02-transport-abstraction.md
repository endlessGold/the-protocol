# 05-02 - Transport 추상화 설계

## 개요

The Protocol은 다양한 네트워크 프로토콜(TCP, UDP, WebSocket, HTTP)을 지원하기 위해 Transport 추상화 계층을 도입한다. 모든 Transport는 동일한 인터페이스를 구현하여, 프로토콜 레이어(Protocol Codec)와 독립적으로 동작한다.

## Transport 아키텍처

```
┌─────────────────────────────────────────────────────┐
│                   Application Layer                 │
│              (CommandRouter, GameWorld)              │
├─────────────────────────────────────────────────────┤
│                   Protocol Layer                     │
│           (Message, ProtocolCodec, serialize)        │
├──────────┬──────────┬───────────┬───────────────────┤
│   TCP    │   UDP    │ WebSocket │      HTTP         │
│ Transport│ Transport│ Transport │    Transport      │
└──────────┴──────────┴───────────┴───────────────────┘
```

## Transport Trait 정의

```rust
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{mpsc, broadcast};

use protocol_protocol::Message;

#[derive(Debug, Clone)]
pub struct TransportEvent {
    pub session_id: u64,
    pub event: TransportEventType,
}

pub enum TransportEventType {
    Connected(SocketAddr),
    Disconnected(u64),
    MessageReceived(Message),
    MessageSent(Message),
    Error(String),
}

#[async_trait]
pub trait Transport: Send + Sync {
    /// Transport 타입 식별자
    fn transport_type(&self) -> TransportType;

    /// 리스너 바인딩 (서버 모드)
    async fn bind(&mut self, addr: SocketAddr) -> Result<(), TransportError>;

    /// 연결 수신 대기 (서버 모드)
    async fn accept(&self) -> Result<TransportConnection, TransportError>;

    /// 연결 요청 (클라이언트 모드)
    async fn connect(&self, addr: SocketAddr) -> Result<TransportConnection, TransportError>;

    /// 연결 닫기
    async fn close(&self, connection_id: u64) -> Result<(), TransportError>;

    /// 현재 연결 수
    fn connection_count(&self) -> usize;

    /// 최대 연결 수
    fn max_connections(&self) -> usize;
}

#[derive(Debug, Clone)]
pub struct TransportConnection {
    pub id: u64,
    pub remote_addr: SocketAddr,
    pub transport_type: TransportType,
    pub connected_at: std::time::Instant,
    pub tx: mpsc::Sender<Message>,
    pub rx: mpsc::Receiver<Message>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportType {
    Tcp,
    Udp,
    WebSocket,
    Http,
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Connection closed")]
    Closed,

    #[error("Connection limit reached")]
    LimitReached,

    #[error("Invalid frame")]
    InvalidFrame,

    #[error("Timeout")]
    Timeout,

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("Protocol error: {0}")]
    Protocol(String),
}
```

## TCP Transport 구현 상세

현재 구현된 TCP Transport는 프로토콜 레이어와 직접 통신하며, `NetworkManager`가 이 역할을 수행한다.

### 현재 구조

```rust
// core/network/src/lib.rs - 현재 구현
pub struct NetworkManager {
    tcp_listener: Option<TcpListener>,
    session_manager: Arc<SessionManager>,
    codec: ProtocolCodec,
}
```

### 프레임 포맷 (현재)

```
┌──────────────┬──────────┬──────────┬─────────────┬──────────┬───────────┐
│ Length (4B)  │ Ver (1B) │ ID (8B)  │ Type (1B)   │ Payload  │ CRC32 (4B)│
│   u32 BE     │   u8     │   u64 BE │    u8       │ variable │  u32 BE   │
└──────────────┴──────────┴──────────┴─────────────┴──────────┴───────────┘
```

### TCP 연결 핸드셰이크

```
Client                          Server
  │                               │
  │──── Hello ───────────────────>│
  │     (version, client_type)    │
  │                               │
  │<─── HelloAck ────────────────│
  │     (session_id, server_time) │
  │                               │
  │──── Command ─────────────────>│
  │                               │
  │<─── CommandResponse ─────────│
  │                               │
  │──── Ping ────────────────────>│  (30초 간격)
  │<─── Pong ────────────────────│
  │                               │
  │──── Disconnect ──────────────>│  (종료 시)
```

### 개선된 TCP Transport 설계

```rust
pub struct TcpTransport {
    listener: Option<TcpListener>,
    connections: Arc<DashMap<u64, TcpConnection>>,
    codec: ProtocolCodec,
    config: TcpTransportConfig,
    next_connection_id: AtomicU64,
}

pub struct TcpTransportConfig {
    pub bind_address: String,
    pub max_connections: usize,
    pub nodelay: bool,
    pub keepalive_interval: Option<Duration>,
    pub read_buffer_size: usize,
    pub write_buffer_size: usize,
    pub handshake_timeout: Duration,
}

struct TcpConnection {
    id: u64,
    reader: tokio::io::BufReader<tokio::io::ReadHalf<TcpStream>>,
    writer: tokio::io::WriteHalf<TcpStream>,
    remote_addr: SocketAddr,
}

#[async_trait]
impl Transport for TcpTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    async fn bind(&mut self, addr: SocketAddr) -> Result<(), TransportError> {
        let listener = TcpListener::bind(addr).await?;
        self.listener = Some(listener);
        tracing::info!("TCP Transport listening on {}", addr);
        Ok(())
    }

    async fn accept(&self) -> Result<TransportConnection, TransportError> {
        let listener = self.listener.as_ref()
            .ok_or(TransportError::Closed)?;

        let (stream, addr) = listener.accept().await?;

        if self.connections.len() >= self.config.max_connections {
            return Err(TransportError::LimitReached);
        }

        stream.set_nodelay(self.config.nodelay)?;

        let conn_id = self.next_connection_id.fetch_add(1, Ordering::Relaxed);
        let (reader, writer) = stream.into_split();

        let (client_tx, client_rx) = mpsc::channel(256);
        let (server_tx, server_rx) = mpsc::channel(256);

        let conn = TcpConnection {
            id: conn_id,
            reader: tokio::io::BufReader::new(reader),
            writer,
            remote_addr: addr,
        };

        self.connections.insert(conn_id, conn);

        Ok(TransportConnection {
            id: conn_id,
            remote_addr: addr,
            transport_type: TransportType::Tcp,
            connected_at: std::time::Instant::now(),
            tx: server_tx,
            rx: client_rx,
        })
    }

    async fn connect(&self, addr: SocketAddr) -> Result<TransportConnection, TransportError> {
        let stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(self.config.nodelay)?;

        let conn_id = self.next_connection_id.fetch_add(1, Ordering::Relaxed);
        let (reader, writer) = stream.into_split();

        let (client_tx, client_rx) = mpsc::channel(256);
        let (server_tx, server_rx) = mpsc::channel(256);

        let conn = TcpConnection {
            id: conn_id,
            reader: tokio::io::BufReader::new(reader),
            writer,
            remote_addr: addr,
        };

        self.connections.insert(conn_id, conn);

        Ok(TransportConnection {
            id: conn_id,
            remote_addr: addr,
            transport_type: TransportType::Tcp,
            connected_at: std::time::Instant::now(),
            tx: server_tx,
            rx: client_rx,
        })
    }

    async fn close(&self, connection_id: u64) -> Result<(), TransportError> {
        self.connections.remove(&connection_id)
            .ok_or(TransportError::Closed)?;
        Ok(())
    }

    fn connection_count(&self) -> usize {
        self.connections.len()
    }

    fn max_connections(&self) -> usize {
        self.config.max_connections
    }
}
```

## UDP Transport 설계 (미구현)

UDP는 연결 기반이 아니므로, 시퀀스 넘버와 ACK를 사용하여 신뢰성을 확보한다.

### Datagram 프레이밍

```
┌──────────────┬──────────┬──────────┬─────────────┬──────────────┬───────────┐
│ Session (8B) │ Seq (4B) │ Ack (4B) │ Flags (1B)  │   Payload    │ CRC16 (2B)│
│    u64 BE    │  u32 BE  │  u32 BE  │    u8       │  variable    │  u16 BE   │
└──────────────┴──────────┴──────────┴─────────────┴──────────────┴───────────┘
```

**플래그 비트:**
- bit 0: DATA - 데이터 패킷
- bit 1: ACK - 확인 응답
- bit 2: SYN - 연결 요청
- bit 3: FIN - 연결 종료
- bit 4: RETRANSMIT - 재전송 패킷

### 시퀀스 넘버

```rust
pub struct UdpSession {
    pub session_id: u64,
    pub remote_addr: SocketAddr,
    pub local_seq: AtomicU32,      // 로컬 시퀀스 번호
    pub remote_seq: AtomicU32,     // 원격 시퀀스 번호
    pub ack_seq: AtomicU32,        // 마지막 확인된 시퀀스
    pub send_buffer: DashMap<u32, SentPacket>,  // 미확인 패킷 버퍼
    pub recv_buffer: DashMap<u32, ReceivedPacket>, // 수신 순서 재조립 버퍼
}

struct SentPacket {
    data: Vec<u8>,
    sent_at: Instant,
    retry_count: u32,
}

struct ReceivedPacket {
    data: Vec<u8>,
    received_at: Instant,
    ordered: bool,
}
```

### ACK/재전송

```rust
pub struct UdpReliability {
    session: UdpSession,
    config: ReliabilityConfig,
}

pub struct ReliabilityConfig {
    pub max_retries: u32,          // 최대 재전송 횟수: 5
    pub base_timeout: Duration,    // 기본 타임아웃: 200ms
    pub max_timeout: Duration,     // 최대 타임아웃: 5000ms
    pub ack_timeout: Duration,     // ACK 대기 타임아웃: 100ms
    pub window_size: u32,          // 동시 미확인 패킷 수: 64
}

impl UdpReliability {
    /// 패킷 전송 (신뢰성 보장)
    pub async fn send(&self, data: Vec<u8>) -> Result<(), TransportError> {
        let seq = self.session.local_seq.fetch_add(1, Ordering::SeqCst);

        // 패킷 생성
        let packet = UdpPacket {
            session_id: self.session.session_id,
            seq,
            ack: self.session.ack_seq.load(Ordering::SeqCst),
            flags: UdpFlags::DATA,
            payload: data.clone(),
        };

        // 전송 버퍼에 저장
        self.session.send_buffer.insert(seq, SentPacket {
            data,
            sent_at: Instant::now(),
            retry_count: 0,
        });

        // UDP 전송
        self.send_raw(&packet).await?;

        Ok(())
    }

    /// ACK 전송
    pub async fn send_ack(&self, ack_seq: u32) -> Result<(), TransportError> {
        let packet = UdpPacket {
            session_id: self.session.session_id,
            seq: self.session.local_seq.load(Ordering::SeqCst),
            ack: ack_seq,
            flags: UdpFlags::ACK,
            payload: vec![],
        };

        self.send_raw(&packet).await?;
        Ok(())
    }

    /// 재전송 타이머 실행
    pub async fn retransmit_loop(&self) {
        loop {
            tokio::time::sleep(self.config.ack_timeout).await;

            let now = Instant::now();
            for mut entry in self.session.send_buffer.iter_mut() {
                let packet = entry.value_mut();
                let elapsed = now.duration_since(packet.sent_at);

                if elapsed > self.config.base_timeout {
                    if packet.retry_count < self.config.max_retries {
                        // 재전송
                        packet.retry_count += 1;
                        packet.sent_at = now;
                        tracing::debug!(
                            seq = entry.key(),
                            retry = packet.retry_count,
                            "UDP retransmit"
                        );
                    } else {
                        // 최대 재시도 초과 - 연결 끊김
                        tracing::warn!(
                            seq = entry.key(),
                            "UDP packet lost after max retries"
                        );
                    }
                }
            }
        }
    }
}
```

### 패킷 조립

```rust
pub struct UdpPacketAssembler {
    recv_buffer: BTreeMap<u32, Vec<u8>>,
    expected_seq: u32,
}

impl UdpPacketAssembler {
    pub fn new() -> Self {
        Self {
            recv_buffer: BTreeMap::new(),
            expected_seq: 0,
        }
    }

    /// 수신된 패킷을 버퍼에 추가하고, 순서대로 조립된 데이터 반환
    pub fn receive(&mut self, seq: u32, data: Vec<u8>, is_last: bool) -> Option<Vec<u8>> {
        self.recv_buffer.insert(seq, data);

        // 연속된 시퀀스 번호 확인
        let mut assembled = Vec::new();
        while let Some(data) = self.recv_buffer.remove(&self.expected_seq) {
            assembled.extend_from_slice(&data);
            self.expected_seq += 1;
        }

        if assembled.is_empty() {
            None
        } else {
            Some(assembled)
        }
    }
}
```

## WebSocket Transport 설계 (미구현)

WebSocket은 HTTP 업그레이드 핸드셰이크를 통해 연결을 설정한다.

### 업그레이드 핸드셰이크

```
Client                          Server
  │                               │
  │──── HTTP GET /ws ────────────>│
  │     Upgrade: websocket        │
  │     Sec-WebSocket-Key: xxx    │
  │                               │
  │<─── 101 Switching Protocols ──│
  │     Upgrade: websocket        │
  │     Sec-WebSocket-Accept: yyy │
  │                               │
  │════ WebSocket Connection ════│
  │     (binary frames)           │
```

### 바이너리 프레임

```rust
pub struct WebSocketTransport {
    listener: Option<tokio_tungstenite::WebSocketStream<TcpStream>>,
    connections: Arc<DashMap<u64, WebSocketConnection>>,
    config: WebSocketConfig,
}

struct WebSocketConnection {
    id: u64,
    ws_stream: SplitSink<WebSocketStream<TcpStream>, Message>,
    remote_addr: SocketAddr,
}

// WebSocket은 프레임 레이어를 자체적으로 관리하므로
// The Protocol의 바이너리 프레임을 WebSocket binary frame으로 전송
// 별도의 Length Prefix 프레이밍 불필요

impl WebSocketTransport {
    pub async fn accept_connection(
        &self,
        stream: TcpStream,
    ) -> Result<WebSocketConnection, TransportError> {
        let ws_stream = tokio_tungstenite::accept_async(stream).await?;
        let (write, read) = ws_stream.split();

        let conn_id = self.next_connection_id();

        Ok(WebSocketConnection {
            id: conn_id,
            ws_stream: write,
            remote_addr: stream.peer_addr()?,
        })
    }
}
```

### WebSocket vs TCP 비교

| 특성 | TCP | WebSocket |
|------|-----|-----------|
| 프레이밍 | Length Prefix | WebSocket Frame |
| 업그레이드 | 불필요 | HTTP 업그레이드 |
| 보안 | TLS (별도) | WSS (TLS 내장) |
| 브라우저 지원 | 없음 | 있음 |
| 프록시 우회 | 어려움 | 쉬움 |
| 성능 | 높음 | 중간 |

## HTTP Transport 설계 (미구현)

HTTP REST API 엔드포인트와 WebSocket 업그레이드를 지원한다.

### REST API 엔드포인트

```rust
use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::State,
};

pub struct HttpTransport {
    app: Router,
    state: AppState,
}

pub struct AppState {
    session_manager: Arc<SessionManager>,
    game_world: Arc<RwLock<GameWorld>>,
    auth_manager: Arc<AuthManager>,
}

impl HttpTransport {
    pub fn new(state: AppState) -> Self {
        let app = Router::new()
            // REST API
            .route("/api/v1/auth/login", post(handlers::auth::login))
            .route("/api/v1/auth/register", post(handlers::auth::register))
            .route("/api/v1/characters/:id", get(handlers::characters::get))
            .route("/api/v1/characters", post(handlers::characters::create))
            .route("/api/v1/inventory/:character_id", get(handlers::inventory::get))
            .route("/api/v1/ranking", get(handlers::ranking::get))
            // WebSocket 업그레이드
            .route("/ws", get(handlers::ws::upgrade))
            .with_state(state);

        Self { app, state }
    }

    pub async fn serve(&self, addr: SocketAddr) -> Result<(), TransportError> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, self.app.clone()).await?;
        Ok(())
    }
}
```

### WebSocket 업그레이드

```rust
// HTTP → WebSocket 업그레이드 핸들러
pub async fn upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut write, mut read) = socket.split();

    // WebSocket 연결을 TransportConnection으로 변환
    // 이후 TCP와 동일한 프로토콜 처리 파이프라인 사용
}
```

## Transport 레지스트리

```rust
pub struct TransportRegistry {
    transports: Arc<DashMap<TransportType, Arc<dyn Transport>>>,
}

impl TransportRegistry {
    pub fn new() -> Self {
        Self {
            transports: Arc::new(DashMap::new()),
        }
    }

    pub fn register(&self, transport: Arc<dyn Transport>) {
        let t = transport.transport_type();
        self.transports.insert(t, transport);
        tracing::info!("Registered transport: {:?}", t);
    }

    pub fn get(&self, transport_type: &TransportType) -> Option<Arc<dyn Transport>> {
        self.transports.get(transport_type).map(|t| t.value().clone())
    }

    pub fn active_count(&self) -> usize {
        self.transports.values()
            .map(|t| t.connection_count())
            .sum()
    }
}
```

## 세션과 Transport 연결

```rust
// SessionManager가 Transport 유형을 인식
impl SessionManager {
    pub fn create_session(
        &self,
        addr: SocketAddr,
        transport: TransportType,
    ) -> Result<u64, SessionError> {
        let session_id = self.next_id();
        let (tx, rx) = mpsc::channel(256);

        let session = Session::new(session_id, addr, transport, tx, rx);
        self.sessions.insert(session_id, session);

        tracing::info!(
            session_id,
            remote_addr = %addr,
            transport = ?transport,
            "Session created"
        );

        Ok(session_id)
    }
}
```

## 미래 확장: 멀티 Transport 시나리오

```
┌───────────────────────────────────────────────────┐
│                    Gateway                        │
│  ┌──────┐  ┌──────┐  ┌───────────┐  ┌─────────┐ │
│  │ TCP  │  │ UDP  │  │ WebSocket │  │  HTTP   │ │
│  │ :7770│  │ :7771│  │   :7772   │  │  :7773  │ │
│  └──┬───┘  └──┬───┘  └─────┬─────┘  └────┬────┘ │
│     └─────────┴────────────┴──────────────┘      │
│                        │                          │
│              ┌─────────▼─────────┐               │
│              │  Protocol Codec   │               │
│              │  (공통 처리)       │               │
│              └─────────┬─────────┘               │
│                        │                          │
│              ┌─────────▼─────────┐               │
│              │  Session Manager  │               │
│              └───────────────────┘               │
└───────────────────────────────────────────────────┘
```

## Transport 선택 기준

| 시나리오 | 권장 Transport | 이유 |
|---------|---------------|------|
| 일반 게임 클라이언트 | TCP | 안정성, 순서 보장 |
| 실시간 전투 | UDP | 낮은 지연 시간 |
| 브라우저 클라이언트 | WebSocket | 브라우저 네이티브 지원 |
| REST API/모바일 | HTTP | 범용성, 프록시 호환 |
| 관리 도구 | HTTP | 간편한 통합 |
