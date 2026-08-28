# 05-01 - 미들웨어/필터 체인 설계

## 개요

The Protocol의 네트워크 레이어는 미들웨어 체인(Middleware Chain) 패턴을 통해 요청/응답 처리 파이프라인을 구성한다. 미들웨어는 인터셉터(Interceptor) 패턴을 기반으로 하며, 각 미들웨어가 독립적인 관심사를 담당하여 단일 책임 원칙을 준수한다.

## 미들웨어 아키텍처

```
┌─────────────────────────────────────────────────────────────┐
│                     클라이언트 요청                           │
│                           │                                 │
│                    ┌──────▼──────┐                          │
│                    │   Logging   │  요청 시작 시간 기록       │
│                    │ Middleware  │                           │
│                    └──────┬──────┘                          │
│                           │                                 │
│                    ┌──────▼──────┐                          │
│                    │  Rate Limit │  토큰 버킷 과금           │
│                    │ Middleware  │                           │
│                    └──────┬──────┘                          │
│                           │                                 │
│                    ┌──────▼──────┐                          │
│                    │     Auth    │  세션 인증 확인           │
│                    │ Middleware  │                           │
│                    └──────┬──────┘                          │
│                           │                                 │
│                    ┌──────▼──────┐                          │
│                    │ Validation  │  메시지 크기/포맷 검증     │
│                    │ Middleware  │                           │
│                    └──────┬──────┘                          │
│                           │                                 │
│                    ┌──────▼──────┐                          │
│                    │   Handler   │  비즈니스 로직 실행        │
│                    │             │                           │
│                    └──────┬──────┘                          │
│                           │                                 │
│                    ┌──────▼──────┐                          │
│                    │  Response   │  응답 후처리              │
│                    │ Middleware  │                           │
│                    └─────────────┘                          │
│                           │                                 │
└───────────────────────────┼─────────────────────────────────┘
```

## 인터셉터 패턴

미들웨어는 요청을 가로채고, 다음 핸들러를 호출한 후, 응답을 후처리하는 인터셉터로 동작한다.

```rust
use std::sync::Arc;
use tokio::sync::mpsc;

use protocol_protocol::{Message, MessageType};

#[derive(Debug, Clone)]
pub struct MiddlewareContext {
    pub session_id: u64,
    pub message: Message,
    pub started_at: std::time::Instant,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug)]
pub enum MiddlewareAction {
    Continue,
    Reject(String),
}

#[async_trait::async_trait]
pub trait Middleware: Send + Sync {
    async fn on_request(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<MiddlewareAction, MiddlewareError>;

    async fn on_response(
        &self,
        ctx: &mut MiddlewareContext,
        response: &mut Message,
    ) -> Result<(), MiddlewareError>;

    fn name(&self) -> &str;
    fn priority(&self) -> i32;
}
```

## 미들웨어 체인

```rust
pub struct MiddlewareChain {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareChain {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    pub fn register(&mut self, middleware: Arc<dyn Middleware>) {
        self.middlewares.push(middleware);
        self.middlewares.sort_by_key(|m| m.priority());
    }

    pub fn remove(&mut self, name: &str) {
        self.middlewares.retain(|m| m.name() != name);
    }

    pub async fn execute(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<Message, MiddlewareError> {
        // 1. 요청 미들웨어 실행 (순서대로)
        for mw in &self.middlewares {
            match mw.on_request(ctx).await? {
                MiddlewareAction::Continue => continue,
                MiddlewareAction::Reject(reason) => {
                    return Ok(Message::error(reason));
                }
            }
        }

        // 2. 핸들러 실행 (실제 비즈니스 로직)
        let mut response = self.execute_handler(ctx).await?;

        // 3. 응답 미들웨어 실행 (역순)
        for mw in self.middlewares.iter().rev() {
            mw.on_response(ctx, &mut response).await?;
        }

        Ok(response)
    }

    async fn execute_handler(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<Message, MiddlewareError> {
        // 핸들러 로직 (CommandRouter 등에서 호출)
        todo!()
    }
}
```

## 각 미들웨어 상세

### LoggingMiddleware

요청/응답 전 과정을 로깅한다. 모든 요청의 시작/종료 시간, 메시지 타입, 세션 ID, 처리 시간을 기록한다.

```rust
pub struct LoggingMiddleware;

#[async_trait::async_trait]
impl Middleware for LoggingMiddleware {
    async fn on_request(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<MiddlewareAction, MiddlewareError> {
        tracing::info!(
            session_id = ctx.session_id,
            message_type = ?ctx.message.message_type,
            message_id = ctx.message.id,
            "요청 수신 시작"
        );
        Ok(MiddlewareAction::Continue)
    }

    async fn on_response(
        &self,
        ctx: &mut MiddlewareContext,
        response: &Message,
    ) -> Result<(), MiddlewareError> {
        let elapsed = ctx.started_at.elapsed();
        tracing::info!(
            session_id = ctx.session_id,
            message_id = ctx.message.id,
            response_type = ?response.message_type,
            elapsed_ms = elapsed.as_millis() as u64,
            "응답 전송 완료"
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "logging"
    }

    fn priority(&self) -> i32 {
        100  // 가장 높은 우선순위 (가장 먼저 실행)
    }
}
```

**로깅 포맷:**
```
2026-08-28T10:00:00Z INFO request_start session_id=1 message_type=Command message_id=12345
2026-08-28T10:00:00Z INFO request_complete session_id=1 message_id=12345 elapsed_ms=2
```

### RateLimitMiddleware

토큰 버킷 알고리즘(Token Bucket Algorithm)을 사용하여 클라이언트별 요청 빈도를 제한한다. 각 세션은 고유한 버킷을 가지며, 초당 허용 요청 수(RPS)와 버킷 크기를 설정할 수 있다.

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant};

pub struct TokenBucket {
    pub tokens: f64,
    pub max_tokens: f64,
    pub refill_rate: f64,  // 초당 리필되는 토큰 수
    pub last_refill: Instant,
}

impl TokenBucket {
    pub fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    pub fn try_consume(&mut self, tokens: f64) -> bool {
        self.refill();
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }
}

pub struct RateLimitMiddleware {
    buckets: Arc<RwLock<HashMap<u64, TokenBucket>>>,
    max_tokens: f64,
    refill_rate: f64,
}

impl RateLimitMiddleware {
    pub fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            buckets: Arc::new(RwLock::new(HashMap::new())),
            max_tokens,
            refill_rate,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for RateLimitMiddleware {
    async fn on_request(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<MiddlewareAction, MiddlewareError> {
        let mut buckets = self.buckets.write().await;
        let bucket = buckets
            .entry(ctx.session_id)
            .or_insert_with(|| TokenBucket::new(self.max_tokens, self.refill_rate));

        if bucket.try_consume(1.0) {
            Ok(MiddlewareAction::Continue)
        } else {
            tracing::warn!(
                session_id = ctx.session_id,
                "Rate limit exceeded"
            );
            Ok(MiddlewareAction::Reject(
                "Rate limit exceeded. Please slow down.".to_string(),
            ))
        }
    }

    async fn on_response(
        &self,
        _ctx: &mut MiddlewareContext,
        _response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    fn name(&self) -> &str {
        "rate_limit"
    }

    fn priority(&self) -> i32 {
        90  // Logging 다음
    }
}
```

**토큰 버킷 알고리즘 동작 원리:**
- 버킷은 최대 `max_tokens`만큼의 토큰을 보유
- 매 초마다 `refill_rate`만큼의 토큰이 리필됨
- 요청 시 토큰 1개를 소비하며, 토큰이 없으면 요청 거부
- 네트워크 지연이나 일시적 부하 증가에 유연하게 대응

**기본 설정:**
| 설정 | 값 | 설명 |
|------|-----|------|
| max_tokens | 10.0 | 버킷 최대 토큰 수 |
| refill_rate | 5.0/초 | 초당 리필 토큰 수 |
| burst | 10 | 최대 버스트 허용 |

### AuthMiddleware

세션 인증 상태를 확인한다. 인증되지 않은 세션은 일부 커맨드(look 제외)에 접근할 수 없다.

```rust
use protocol_session::{SessionManager, SessionState};

pub struct AuthMiddleware {
    session_manager: Arc<SessionManager>,
    public_commands: Vec<String>,
}

impl AuthMiddleware {
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        let public_commands = vec![
            "login".to_string(),
            "register".to_string(),
            "hello".to_string(),
            "ping".to_string(),
        ];

        Self {
            session_manager,
            public_commands,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for AuthMiddleware {
    async fn on_request(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<MiddlewareAction, MiddlewareError> {
        // 공개 커맨드는 인증 불필요
        let cmd_type = ctx.message.payload.get(0)
            .and_then(|b| std::str::from_utf8(&[*b]).ok())
            .unwrap_or("");

        if self.public_commands.iter().any(|c| c == cmd_type) {
            return Ok(MiddlewareAction::Continue);
        }

        // 세션 인증 상태 확인
        if let Some(session) = self.session_manager.get(ctx.session_id) {
            if session.state >= SessionState::Authenticated {
                Ok(MiddlewareAction::Continue)
            } else {
                Ok(MiddlewareAction::Reject(
                    "Authentication required. Please login first.".to_string(),
                ))
            }
        } else {
            Ok(MiddlewareAction::Reject(
                "Session not found. Please reconnect.".to_string(),
            ))
        }
    }

    async fn on_response(
        &self,
        _ctx: &mut MiddlewareContext,
        _response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    fn name(&self) -> &str {
        "auth"
    }

    fn priority(&self) -> i32 {
        80
    }
}
```

### ValidationMiddleware

들어오는 메시지의 크기, 포맷, 필수 필드를 검증한다. 비정상적인 메시지를 조기에 차단하여 서버 리소스를 보호한다.

```rust
pub struct ValidationMiddleware {
    max_message_size: usize,
    max_payload_size: usize,
    allowed_versions: Vec<u8>,
}

impl ValidationMiddleware {
    pub fn new() -> Self {
        Self {
            max_message_size: 1024 * 1024,    // 1MB
            max_payload_size: 512 * 1024,     // 512KB
            allowed_versions: vec![1],         // 프로토콜 v1
        }
    }
}

#[async_trait::async_trait]
impl Middleware for ValidationMiddleware {
    async fn on_request(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<MiddlewareAction, MiddlewareError> {
        // 1. 프로토콜 버전 검증
        if !self.allowed_versions.contains(&ctx.message.version) {
            return Ok(MiddlewareAction::Reject(format!(
                "Unsupported protocol version: {}",
                ctx.message.version
            )));
        }

        // 2. 메시지 전체 크기 검증
        let total_size = 14 + ctx.message.payload.len() + 4; // header + payload + checksum
        if total_size > self.max_message_size {
            return Ok(MiddlewareAction::Reject(format!(
                "Message too large: {} bytes (max: {})",
                total_size, self.max_message_size
            )));
        }

        // 3. 페이로드 크기 검증
        if ctx.message.payload.len() > self.max_payload_size {
            return Ok(MiddlewareAction::Reject(format!(
                "Payload too large: {} bytes (max: {})",
                ctx.message.payload.len(),
                self.max_payload_size
            )));
        }

        // 4. 메시지 ID 검증 (0은 유효하지 않음)
        if ctx.message.id == 0 {
            return Ok(MiddlewareAction::Reject(
                "Invalid message ID: 0".to_string(),
            ));
        }

        Ok(MiddlewareAction::Continue)
    }

    async fn on_response(
        &self,
        _ctx: &mut MiddlewareContext,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        // 응답도 동일한 검증 적용
        let total_size = 14 + response.payload.len() + 4;
        if total_size > self.max_message_size {
            *response = Message::error("Response too large".to_string());
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "validation"
    }

    fn priority(&self) -> i32 {
        70
    }
}
```

### CompressionMiddleware (미구현)

gzip/zstd 압축을 지원하는 미들웨어. 대용량 응답(월드 데이터, 인벤토리 등)의 대역폭을 절감한다.

```rust
pub struct CompressionMiddleware {
    min_size_for_compression: usize,
    algorithm: CompressionAlgorithm,
}

pub enum CompressionAlgorithm {
    Gzip,
    Zstd,
}

impl CompressionMiddleware {
    pub fn new(algorithm: CompressionAlgorithm) -> Self {
        Self {
            min_size_for_compression: 1024,  // 1KB 이상일 때만 압축
            algorithm,
        }
    }
}

// 미구현: 핸드셰이크에서 클라이언트 지원 압축 알고리즘 협상 필요
// HelloAck의 capabilities 필드에 "gzip", "zstd" 등 포함
// 요청 헤더에 Content-Encoding 필드 추가 필요
```

**압축 대상:**
- `LookResponse` (방 정보 + NPC 목록)
- `InventoryResponse` (아이템 목록)
- `WorldState` (전체 월드 상태)

### EncryptionMiddleware (미구현)

TLS 암호화를 담당하는 미들웨어. 현재 TCP 연결은 평문으로 전송되며, 향후 TLS 지원이 추가될 예정이다.

```rust
pub struct EncryptionMiddleware {
    cert_path: Option<String>,
    key_path: Option<String>,
}

// 미구현: rustls 또는 openssl을 사용한 TLS 구현 필요
// TCP 리스너 수준에서 TLS 적용 (TlsListener)
// 또는 대체: 메시지 레이어 암호화 (AES-256-GCM)
```

**보안 고려사항:**
- JWT 토큰은 TLS 없이도 안전하게 전송 (énération 검증)
- 게임 데이터는 암호화 불필요 (공개 데이터)
- 결제/인증 관련 데이터만 TLS 필수

## 미들웨어 등록/제거

```rust
pub struct MiddlewareManager {
    chain: MiddlewareChain,
}

impl MiddlewareManager {
    pub fn new() -> Self {
        let mut chain = MiddlewareChain::new();

        // 기본 미들웨어 등록 (우선순위 순)
        chain.register(Arc::new(LoggingMiddleware));
        chain.register(Arc::new(RateLimitMiddleware::new(10.0, 5.0)));
        chain.register(Arc::new(ValidationMiddleware::new()));

        Self { chain }
    }

    pub fn with_auth(mut self, session_manager: Arc<SessionManager>) -> Self {
        self.chain.register(Arc::new(AuthMiddleware::new(session_manager)));
        self
    }

    pub fn with_compression(mut self, algorithm: CompressionAlgorithm) -> Self {
        self.chain.register(Arc::new(CompressionMiddleware::new(algorithm)));
        self
    }

    pub fn remove(&mut self, name: &str) {
        self.chain.remove(name);
    }
}
```

## 미들웨어 우선순위

각 미들웨어는 고유한 우선순위 값을 가지며, 값이 작을수록 먼저 실행된다.

| 우선순위 | 미들웨어 | 설명 |
|---------|----------|------|
| 100 | LoggingMiddleware | 모든 요청/응답 로깅 |
| 90 | RateLimitMiddleware | 요청 빈도 제한 |
| 80 | AuthMiddleware | 인증 상태 확인 |
| 70 | ValidationMiddleware | 메시지 유효성 검증 |
| 60 | CompressionMiddleware | 응답 압축 (미구현) |
| 50 | EncryptionMiddleware | TLS 암호화 (미구현) |
| 0 | Handler | 실제 비즈니스 로직 |

## 미들웨어 컨텍스트 확장

미들웨어 간 데이터 공유를 위한 메타데이터 스토어:

```rust
impl MiddlewareContext {
    pub fn set<T: Send + Sync + 'static>(&mut self, key: &str, value: T) {
        self.metadata.insert(key.to_string(), /* boxing */);
    }

    pub fn get<T: Send + Sync + 'static>(&self, key: &str) -> Option<&T> {
        self.metadata.get(key).and_then(|v| v.downcast_ref())
    }
}
```

## 에러 처리

```rust
#[derive(Debug, thiserror::Error)]
pub enum MiddlewareError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Codec error: {0}")]
    Codec(#[from] protocol_protocol::codec::CodecError),

    #[error("Session error: {0}")]
    Session(#[from] protocol_session::SessionError),

    #[error("Middleware '{0}' rejected: {1}")]
    Rejected(String, String),

    #[error("Internal error: {0}")]
    Internal(String),
}
```

## 성능 고려사항

- **미들웨어 수**: 최대 10개로 제한 (오버헤드 방지)
- **동기 락 회피**: `RwLock` 대신 `DashMap` 사용
- **버킷 정리**: 비활성 세션의 토큰 버킷은 주기적으로 정리 (Scheduler 활용)
- **로깅 최적화`: 로그 레벨에 따른 조건부 로깅 (tracing filter 활용)
