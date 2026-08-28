# Event Bus 시스템 설계 (미구현)

> **상태: ❌ 미구현** — 현재 `domain/src/event.rs`에 `DomainEvent` enum만 정의되어 있고, 발행/구독 메커니즘은 없음.

---

## 1. 이벤트 버스 아키텍처

### 1.1 왜 Event Bus인가?

현재 시스템에서 이벤트는 도메인 계층(`DomainEvent`)에서 생성되지만 처리되지 않는다:

```rust
// domain/src/character.rs — 이벤트 생성
pub fn gain_experience(&mut self, xp: u64) -> Vec<DomainEvent> {
    let mut events = Vec::new();
    // ...
    events.push(DomainEvent::LevelUp { character_id, new_level });
    events  // 반환만 하고 소비 안 함
}
```

Event Bus를 도입하면:

| 문제 | 해결 |
|------|------|
| 서비스 간 결합도 높음 | 구독 기반 느슨한 결합 |
| 부가 로직 삽입 시 서비스 수정 필요 | Handler 추가만으로 확장 |
| 로깅/알림/통계 삽입 어려움 | 이벤트 기반 비동기 처리 |
| 멀티플레이어 동기화 어려움 | 이벤트 브로드캐스트 |

### 1.2 전체 아키텍처

```
┌──────────────────────────────────────────────────┐
│                Domain Layer                       │
│  Character.gain_experience() → DomainEvent       │
│  Combat.process_attack() → DomainEvent            │
└──────────────────────┬───────────────────────────┘
                       │ (emit)
                       ▼
┌──────────────────────────────────────────────────┐
│               Event Bus                           │
│  ┌─────────────────────────────────────────────┐ │
│  │  In-Process Bus (tokio::broadcast)          │ │
│  │  - 같은 프로세스 내 이벤트                    │ │
│  │  - 낮은 지연, 높은 처리량                    │ │
│  └─────────────────────────────────────────────┘ │
│  ┌─────────────────────────────────────────────┐ │
│  │  분산 버스 (Redis Pub/Sub)                   │ │
│  │  - 멀티 인스턴스 이벤트                      │ │
│  │  - 느슨한 일관성, 높은 가용성                │ │
│  └─────────────────────────────────────────────┘ │
└──────────────────────┬───────────────────────────┘
                       │ (dispatch)
          ┌────────────┼────────────┐
          ▼            ▼            ▼
   ┌──────────┐ ┌──────────┐ ┌──────────┐
   │ Logging  │ │ Notify   │ │ Sync     │
   │ Handler  │ │ Handler  │ │ Handler  │
   └──────────┘ └──────────┘ └──────────┘
```

---

## 2. EventBus 인터페이스

### 2.1 핵심 Trait

```rust
use async_trait::async_trait;
use std::collections::HashMap;

/// 이벤트 타입 식별자
pub type EventType = String;

/// 구독 ID
pub type SubscriptionId = u64;

/// 이벤트 핸들러 시그니처
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// 처리할 이벤트 타입 반환
    fn event_type(&self) -> &str;

    /// 이벤트 처리
    async fn handle(&self, event: &DomainEvent) -> Result<(), EventError>;

    /// 핸들러 우선순위 (낮을수록 먼저 실행)
    fn priority(&self) -> i32 { 0 }

    /// 필터링 조건 (선택)
    fn filter(&self, _event: &DomainEvent) -> bool { true }
}

/// 이벤트 버스 인터페이스
#[async_trait]
pub trait EventBus: Send + Sync {
    /// 이벤트 발행
    async fn publish(&self, event: DomainEvent) -> Result<(), EventError>;

    /// 이벤트 핸들러 등록
    fn subscribe(&self, handler: Arc<dyn EventHandler>) -> SubscriptionId;

    /// 구독 해제
    fn unsubscribe(&self, id: SubscriptionId) -> Result<(), EventError>;

    /// 특정 타입의 이벤트에 대한 핸들러 수
    fn handler_count(&self, event_type: &str) -> usize;

    /// 이벤트 처리 대기 (graceful shutdown)
    async fn drain(&self, timeout: std::time::Duration) -> Result<(), EventError>;
}
```

### 2.2 EventError 정의

```rust
#[derive(Debug, Error)]
pub enum EventError {
    #[error("Handler error: {handler} - {message}")]
    HandlerError { handler: String, message: String },

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Bus closed")]
    BusClosed,

    #[error("Subscription not found: {0}")]
    SubscriptionNotFound(SubscriptionId),

    #[error("Channel full")]
    ChannelFull,

    #[error("Timeout waiting for event processing")]
    Timeout,
}
```

---

## 3. 이벤트 핸들러 체인

### 3.1 핸들러 체인 아키텍처

```
DomainEvent 발생
       │
       ▼
┌─────────────────────────┐
│  Priority 0: 로깅       │  ← 모든 이벤트 기록
│  (LoggingHandler)       │
└──────────┬──────────────┘
           │
┌──────────▼──────────────┐
│  Priority 10: 검증      │  ← 이벤트 유효성 검사
│  (ValidationHandler)    │
└──────────┬──────────────┘
           │
┌──────────▼──────────────┐
│  Priority 20: 비즈니스  │  ← 캐릭터 레벨업 보상
│  (LevelUpHandler)       │
└──────────┬──────────────┘
           │
┌──────────▼──────────────┐
│  Priority 30: 알림      │  ← 채팅 메시지 발송
│  (NotificationHandler)  │
└──────────┬──────────────┘
           │
┌──────────▼──────────────┐
│  Priority 40: 동기화    │  ← 멀티플레이어 브로드캐스트
│  (SyncHandler)          │
└──────────┬──────────────┘
           │
┌──────────▼──────────────┐
│  Priority 50: 영속화    │  ← DB 저장
│  (PersistenceHandler)   │
└─────────────────────────┘
```

### 3.2 핸들러 구현 예시

```rust
/// 로깅 핸들러
pub struct LoggingHandler;

#[async_trait]
impl EventHandler for LoggingHandler {
    fn event_type(&self) -> &str { "*" } // 모든 이벤트

    async fn handle(&self, event: &DomainEvent) -> Result<(), EventError> {
        match event {
            DomainEvent::CharacterCreated { character_id, name } => {
                tracing::info!(character_id, name, "Character created");
            }
            DomainEvent::LevelUp { character_id, new_level } => {
                tracing::info!(character_id, new_level, "Level up");
            }
            DomainEvent::CombatStarted { combat_id, attacker_id, target_id } => {
                tracing::info!(combat_id, attacker_id, target_id, "Combat started");
            }
            DomainEvent::AttackExecuted { combat_id, attacker_id, target_id, damage } => {
                tracing::info!(combat_id, attacker_id, target_id, damage, "Attack executed");
            }
            DomainEvent::CombatEnded { combat_id, winner_id, loser_id } => {
                tracing::info!(combat_id, winner_id, loser_id, "Combat ended");
            }
            DomainEvent::PlayerEnteredRoom { player_id, room_id } => {
                tracing::info!(player_id, room_id, "Player entered room");
            }
            DomainEvent::PlayerLeftRoom { player_id, room_id } => {
                tracing::info!(player_id, room_id, "Player left room");
            }
            DomainEvent::ItemAcquired { player_id, item_id, quantity } => {
                tracing::info!(player_id, item_id, quantity, "Item acquired");
            }
            DomainEvent::ItemRemoved { player_id, item_id, quantity } => {
                tracing::info!(player_id, item_id, quantity, "Item removed");
            }
        }
        Ok(())
    }

    fn priority(&self) -> i32 { 0 }
}

/// 레벨업 보상 핸들러
pub struct LevelUpHandler;

#[async_trait]
impl EventHandler for LevelUpHandler {
    fn event_type(&self) -> &str { "LevelUp" }

    async fn handle(&self, event: &DomainEvent) -> Result<(), EventError> {
        if let DomainEvent::LevelUp { character_id, new_level } = event {
            tracing::info!(character_id, new_level, "Applying level up rewards");

            // 최대 HP 증가 (이미 도메인에서 처리됨)
            // 추가 보상: 스폰 포인트, 스킬 포인트 등
            // 향후: 채팅 알림, 파티원에게 알림
        }
        Ok(())
    }

    fn priority(&self) -> i32 { 20 }
}

/// 멀티플레이어 동기화 핸들러
pub struct SyncHandler {
    session_manager: Arc<SessionManager>,
}

#[async_trait]
impl EventHandler for SyncHandler {
    fn event_type(&self) -> &str { "*" }

    async fn handle(&self, event: &DomainEvent) -> Result<(), EventError> {
        match event {
            DomainEvent::PlayerEnteredRoom { player_id, room_id } => {
                // 같은 방의 다른 플레이어에게 전송
                let targets = self.get_room_players(*room_id, *player_id).await;
                for target_id in targets {
                    if let Some(session) = self.session_manager.get_by_player(target_id) {
                        let event_msg = Message::event(Event {
                            id: rand::random(),
                            event_type: "player_entered".to_string(),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            source: "server".to_string(),
                            payload: rmp_serde::to_vec(event).unwrap(),
                            targets: Some(vec![target_id]),
                        });
                        let _ = session.send(event_msg);
                    }
                }
            }
            DomainEvent::AttackExecuted { attacker_id, target_id, damage, .. } => {
                // 공격자/방어자 주변 플레이어에게 전투 이벤트 브로드캐스트
            }
            _ => {}
        }
        Ok(())
    }

    fn priority(&self) -> i32 { 40 }
}
```

---

## 4. 이벤트 필터링

### 4.1 필터 전략

```rust
pub struct EventFilter {
    pub event_types: Option<Vec<String>>,    // 특정 타입만 수신
    pub source_pattern: Option<String>,      // 소스 패턴 매칭
    pub condition: Option<Box<dyn Fn(&DomainEvent) -> bool + Send + Sync>>,
}

impl EventFilter {
    pub fn matches(&self, event: &DomainEvent) -> bool {
        // 타입 필터
        if let Some(ref types) = self.event_types {
            let event_type = match event {
                DomainEvent::CharacterCreated { .. } => "CharacterCreated",
                DomainEvent::LevelUp { .. } => "LevelUp",
                DomainEvent::CombatStarted { .. } => "CombatStarted",
                DomainEvent::AttackExecuted { .. } => "AttackExecuted",
                DomainEvent::CombatEnded { .. } => "CombatEnded",
                DomainEvent::PlayerEnteredRoom { .. } => "PlayerEnteredRoom",
                DomainEvent::PlayerLeftRoom { .. } => "PlayerLeftRoom",
                DomainEvent::ItemAcquired { .. } => "ItemAcquired",
                DomainEvent::ItemRemoved { .. } => "ItemRemoved",
            };
            if !types.iter().any(|t| t == event_type) {
                return false;
            }
        }

        // 컨디션 필터
        if let Some(ref condition) = self.condition {
            if !condition(event) {
                return false;
            }
        }

        true
    }
}
```

### 4.2 필터 사용 예시

```rust
// 전투 관련 이벤트만 수신
let combat_filter = EventFilter {
    event_types: Some(vec![
        "CombatStarted".to_string(),
        "AttackExecuted".to_string(),
        "CombatEnded".to_string(),
    ]),
    source_pattern: None,
    condition: None,
};

// 특정 플레이어 관련 이벤트만 수신
let player_filter = EventFilter {
    event_types: None,
    source_pattern: None,
    condition: Some(Box::new(move |event| {
        match event {
            DomainEvent::LevelUp { character_id, .. } => *character_id == target_player,
            DomainEvent::ItemAcquired { player_id, .. } => *player_id == target_player,
            _ => false,
        }
    })),
};
```

---

## 5. 이벤트 로깅

### 5.1 구조화된 이벤트 로깅

```rust
pub struct AuditLogHandler {
    log_path: String,
}

#[async_trait]
impl EventHandler for AuditLogHandler {
    fn event_type(&self) -> &str { "*" }

    async fn handle(&self, event: &DomainEvent) -> Result<(), EventError> {
        let log_entry = AuditLogEntry {
            timestamp: chrono::Utc::now(),
            event_type: event.type_name(),
            event_data: serde_json::to_value(event)
                .map_err(|e| EventError::Serialization(e.to_string()))?,
        };

        // 파일 로깅 (JSON Lines)
        let log_line = serde_json::to_string(&log_entry).unwrap() + "\n";
        tokio::fs::open(&self.log_path)
            .await?
            .write_all(log_line.as_bytes())
            .await?;

        Ok(())
    }

    fn priority(&self) -> i32 { -10 } // 가장 먼저 실행
}

#[derive(Serialize)]
struct AuditLogEntry {
    timestamp: chrono::DateTime<chrono::Utc>,
    event_type: String,
    event_data: serde_json::Value,
}
```

### 5.2 이벤트 메트릭스

```rust
pub struct MetricsHandler {
    counter: Arc<prometheus::IntCounterVec>,
    histogram: Arc<prometheus::HistogramVec>,
}

impl MetricsHandler {
    pub fn new() -> Self {
        let counter = Arc::new(prometheus::IntCounterVec::new(
            prometheus::opts!("game_events_total", "Total game events"),
            &["event_type"],
        ).unwrap());

        let histogram = Arc::new(prometheus::HistogramVec::new(
            prometheus::histogram_opts!("game_event_duration_seconds", "Event processing duration"),
            &["event_type", "handler"],
        ).unwrap());

        Self { counter, histogram }
    }
}

#[async_trait]
impl EventHandler for MetricsHandler {
    fn event_type(&self) -> &str { "*" }

    async fn handle(&self, event: &DomainEvent) -> Result<(), EventError> {
        let type_name = event.type_name();
        self.counter.with_label_values(&[type_name]).inc();
        Ok(())
    }

    fn priority(&self) -> i32 { -5 }
}
```

---

## 6. In-Process 이벤트 버스 (tokio::broadcast)

### 6.1 구현

```rust
use tokio::sync::broadcast;

pub struct InProcessEventBus {
    sender: broadcast::Sender<DomainEvent>,
    handlers: Arc<RwLock<Vec<RegisteredHandler>>>,
    next_sub_id: AtomicU64,
    metrics: EventBusMetrics,
}

struct RegisteredHandler {
    id: SubscriptionId,
    handler: Arc<dyn EventHandler>,
    filter: Option<EventFilter>,
}

#[derive(Default)]
struct EventBusMetrics {
    published: AtomicU64,
    delivered: AtomicU64,
    errors: AtomicU64,
}

impl InProcessEventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            handlers: Arc::new(RwLock::new(Vec::new())),
            next_sub_id: AtomicU64::new(1),
            metrics: EventBusMetrics::default(),
        }
    }

    /// 이벤트 버스 수신 루프 시작
    pub async fn run(self: Arc<Self>) {
        let mut receiver = self.sender.subscribe();

        loop {
            match receiver.recv().await {
                Ok(event) => {
                    self.metrics.published.fetch_add(1, Ordering::Relaxed);
                    let handlers = self.handlers.read().await;

                    // 우선순위 정렬 후 실행
                    let mut matching_handlers: Vec<_> = handlers.iter()
                        .filter(|h| {
                            h.filter.as_ref()
                                .map(|f| f.matches(&event))
                                .unwrap_or(true)
                        })
                        .collect();

                    matching_handlers.sort_by_key(|h| h.handler.priority());

                    for registered in matching_handlers {
                        if let Err(e) = registered.handler.handle(&event).await {
                            tracing::error!(
                                handler = %registered.handler.event_type(),
                                error = %e,
                                "Event handler failed"
                            );
                            self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                        } else {
                            self.metrics.delivered.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Event bus lagged by {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::info!("Event bus closed");
                    break;
                }
            }
        }
    }
}

#[async_trait]
impl EventBus for InProcessEventBus {
    async fn publish(&self, event: DomainEvent) -> Result<(), EventError> {
        self.sender.send(event)
            .map_err(|_| EventError::BusClosed)?;
        Ok(())
    }

    fn subscribe(&self, handler: Arc<dyn EventHandler>) -> SubscriptionId {
        let id = self.next_sub_id.fetch_add(1, Ordering::Relaxed);
        let registered = RegisteredHandler {
            id,
            handler,
            filter: None,
        };

        // 필터가 있는 핸들러의 경우 별도 관리
        // (실제로는 handlers Vec에 직접 삽입)
        let mut handlers = self.handlers.blocking_write();
        handlers.push(registered);
        id
    }

    fn unsubscribe(&self, id: SubscriptionId) -> Result<(), EventError> {
        let mut handlers = self.handlers.blocking_write();
        handlers.retain(|h| h.id != id);
        Ok(())
    }

    fn handler_count(&self, event_type: &str) -> usize {
        self.handlers.blocking_read().iter()
            .filter(|h| h.handler.event_type() == event_type || h.handler.event_type() == "*")
            .count()
    }

    async fn drain(&self, _timeout: std::time::Duration) -> Result<(), EventError> {
        // In-process 버스는 즉시 종료 가능
        Ok(())
    }
}
```

### 6.2 broadcast 채널 튜닝

| 파라미터 | 기본값 | 권장 | 설명 |
|----------|--------|------|------|
| capacity | 1024 | 4096 | 이벤트 버퍼 크기 |
| lagged threshold | - | 100 | Lagged 경고 기준 |

---

## 7. 분산 이벤트 버스 (Redis Pub/Sub)

### 7.1 아키텍처

```
┌─────────────┐     ┌─────────────┐
│ Instance A  │     │ Instance B  │
│ InProcess   │     │ InProcess   │
│ Bus         │     │ Bus         │
└──────┬──────┘     └──────┬──────┘
       │                   │
       ▼                   ▼
┌─────────────────────────────────┐
│         Redis Pub/Sub           │
│                                 │
│  Channel: game:events           │
│  Channel: game:events:{type}    │
│  Channel: game:sync             │
└─────────────────────────────────┘
```

### 7.2 Redis 기반 분산 버스 구현

```rust
use redis::aio::PubSub;

pub struct RedisEventBus {
    conn: redis::aio::ConnectionManager,
    local_bus: Arc<InProcessEventBus>,
    channel_prefix: String,
}

impl RedisEventBus {
    pub async fn new(
        redis_url: str,
        local_bus: Arc<InProcessEventBus>,
    ) -> Result<Self, EventError> {
        let client = redis::Client::open(redis_url)
            .map_err(|e| EventError::Serialization(e.to_string()))?;
        let conn = client.get_tokio_connection_manager().await
            .map_err(|e| EventError::Serialization(e.to_string()))?;

        Ok(Self {
            conn,
            local_bus,
            channel_prefix: "game:events".to_string(),
        })
    }

    /// Redis Pub/Sub 수신 루프
    pub async fn run(self: Arc<Self>) {
        let mut pubsub = self.conn.as_pubsub();
        pubsub.subscribe(&self.channel_prefix).await.unwrap();

        loop {
            let msg = pubsub.get_message().await;
            match msg {
                Ok(message) => {
                    let payload: String = message.get_payload().unwrap();
                    if let Ok(event) = serde_json::from_str::<DomainEvent>(&payload) {
                        // 로컬 버스로 전파
                        let _ = self.local_bus.publish(event).await;
                    }
                }
                Err(e) => {
                    tracing::error!("Redis Pub/Sub error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }
}

#[async_trait]
impl EventBus for RedisEventBus {
    async fn publish(&self, event: DomainEvent) -> Result<(), EventError> {
        // 1. 로컬 버스에 발행 (즉시 처리)
        self.local_bus.publish(event.clone()).await?;

        // 2. Redis에도 발행 (다른 인스턴스로 전파)
        let payload = serde_json::to_string(&event)
            .map_err(|e| EventError::Serialization(e.to_string()))?;

        self.conn.publish(&self.channel_prefix, &payload).await
            .map_err(|e| EventError::Serialization(e.to_string()))?;

        Ok(())
    }

    fn subscribe(&self, handler: Arc<dyn EventHandler>) -> SubscriptionId {
        self.local_bus.subscribe(handler)
    }

    fn unsubscribe(&self, id: SubscriptionId) -> Result<(), EventError> {
        self.local_bus.unsubscribe(id)
    }

    fn handler_count(&self, event_type: &str) -> usize {
        self.local_bus.handler_count(event_type)
    }

    async fn drain(&self, timeout: std::time::Duration) -> Result<(), EventError> {
        self.local_bus.drain(timeout).await
    }
}
```

### 7.3 Redis 채널 구조

```
game:events                     → 모든 이벤트 (IMARY 채널)
game:events:CharacterCreated    → 캐릭터 생성 이벤트
game:events:LevelUp             → 레벨업 이벤트
game:events:CombatStarted       → 전투 시작 이벤트
game:events:AttackExecuted      → 공격 실행 이벤트
game:events:CombatEnded         → 전투 종료 이벤트
game:events:PlayerEnteredRoom   → 방 입장 이벤트
game:events:PlayerLeftRoom      → 방 퇴장 이벤트
game:events:ItemAcquired        → 아이템 획득 이벤트
game:events:ItemRemoved         → 아이템 제거 이벤트
game:sync                       → 상태 동기화 이벤트
```

---

## 8. 이벤트 드리프트 방지

### 8.1 문제 정의

이벤트 드리프트: 이벤트가 순서대로 처리되지 않거나, 지연되어 최종 상태가 불일치하는 현상.

### 8.2 방지 전략

**전략 1: 이벤트 시퀀스 번호**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencedEvent {
    pub sequence: u64,           // 전역 시퀀스 번호
    pub event: DomainEvent,
    pub timestamp: u64,
    pub source_instance: String, // 생성 인스턴스 ID
}

// 시퀀스 관리
pub struct SequenceManager {
    next_sequence: AtomicU64,
}

impl SequenceManager {
    pub fn next(&self) -> u64 {
        self.next_sequence.fetch_add(1, Ordering::SeqCst)
    }

    pub fn current(&self) -> u64 {
        self.next_sequence.load(Ordering::SeqCst)
    }
}
```

**전략 2: 이벤트 유효 기간**

```rust
impl DomainEvent {
    pub fn max_age(&self) -> std::time::Duration {
        match self {
            DomainEvent::AttackExecuted { .. } => std::time::Duration::from_secs(30),
            DomainEvent::PlayerEnteredRoom { .. } => std::time::Duration::from_secs(60),
            DomainEvent::LevelUp { .. } => std::time::Duration::from_secs(300),
            _ => std::time::Duration::from_secs(60),
        }
    }

    pub fn is_expired(&self) -> bool {
        let age = chrono::Utc::now().timestamp_millis() as u64 - self.timestamp();
        age > self.max_age().as_millis() as u64
    }
}
```

**전략 3: 이벤트 버퍼링 및 배치 처리**

```rust
pub struct EventBatcher {
    buffer: Vec<DomainEvent>,
    max_batch_size: usize,
    flush_interval: std::time::Duration,
}

impl EventBatcher {
    pub async fn run(&mut self, bus: Arc<dyn EventBus>) {
        let mut interval = tokio::time::interval(self.flush_interval);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.flush(&bus).await;
                }
                event = self.receive() => {
                    self.buffer.push(event);
                    if self.buffer.len() >= self.max_batch_size {
                        self.flush(&bus).await;
                    }
                }
            }
        }
    }

    async fn flush(&mut self, bus: &Arc<dyn EventBus>) {
        let batch: Vec<_> = self.buffer.drain(..).collect();
        for event in batch {
            let _ = bus.publish(event).await;
        }
    }
}
```

---

## 9. 이벤트 순서 보장

### 9.1 순서 보장이 필요한 시나리오

| 시나리오 | 순서 요구사항 |
|----------|--------------|
| 전투 턴 | AttackExecuted → CombatEnded 순서 보장 |
| 이동 | PlayerLeftRoom → PlayerEnteredRoom 순서 보장 |
| 레벨업 | ExperienceGained → LevelUp 순서 보장 |
| 아이템 거래 | ItemRemoved → ItemAcquired 순서 보장 |

### 9.2 순서 보장 구현

**방법 1: 토픽 기반 순서 보장**

```rust
// 같은 aggregate에 대한 이벤트는 같은 토픽으로 라우팅
pub fn event_topic(event: &DomainEvent) -> String {
    match event {
        DomainEvent::AttackExecuted { combat_id, .. } |
        DomainEvent::CombatStarted { combat_id, .. } |
        DomainEvent::CombatEnded { combat_id, .. } => {
            format!("combat:{}", combat_id)
        }
        DomainEvent::PlayerEnteredRoom { player_id, .. } |
        DomainEvent::PlayerLeftRoom { player_id, .. } |
        DomainEvent::LevelUp { character_id: player_id, .. } => {
            format!("player:{}", player_id)
        }
        DomainEvent::ItemAcquired { player_id, .. } |
        DomainEvent::ItemRemoved { player_id, .. } => {
            format!("inventory:{}", player_id)
        }
        _ => "global".to_string(),
    }
}
```

**방법 2: Kafka/Redis Streams 사용 (향후)**

```rust
// Redis Streams로 순서 보장
pub async fn publish_ordered(
    &self,
    stream: &str,
    event: &SequencedEvent,
) -> Result<(), EventError> {
    let id = format!("{}-{}", event.source_instance, event.sequence);
    let payload = serde_json::to_string(&event.event).unwrap();

    redis::cmd("XADD")
        .arg(stream)
        .arg(&id)
        .arg("data")
        .arg(&payload)
        .query_async(&mut self.conn)
        .await
        .map_err(|e| EventError::Serialization(e.to_string()))?;

    Ok(())
}
```

---

## 10. 이벤트 버스 초기화 및 통합

### 10.1 전체 초기화

```rust
// runtime/src/main.rs (향후 구조)
async fn run_server(bind: &str, plugin_dir: &str) -> Result<()> {
    // 이벤트 버스 초기화
    let local_bus = Arc::new(InProcessEventBus::new(4096));

    // Redis 연결 시 분산 버스 활성화
    let event_bus: Arc<dyn EventBus> = if let Ok(redis_url) = std::env::var("REDIS_URL") {
        Arc::new(RedisEventBus::new(&redis_url, local_bus.clone()).await?)
    } else {
        local_bus.clone()
    };

    // 핸들러 등록
    event_bus.subscribe(Arc::new(LoggingHandler));
    event_bus.subscribe(Arc::new(AuditLogHandler::new("./logs/audit.jsonl".to_string())));
    event_bus.subscribe(Arc::new(LevelUpHandler));
    event_bus.subscribe(Arc::new(SyncHandler::new(session_manager.clone())));

    // 이벤트 버스 런타임 시작
    let bus_runtime = event_bus.clone();
    tokio::spawn(async move {
        bus_runtime.run().await;
    });

    // ... 서버 나머지 초기화
}
```

### 10.2 의존성 그래프 (Event Bus 포함)

```
CommandRouter
  ├── LookHandler ──────→ GameService ──────→ CharacterRepository
  ├── MoveHandler ──────→ GameService ──────→ WorldRepository
  ├── AttackHandler ────→ GameService ──────→ CombatRepository
  └── ...
                              │
                         DomainEvent 발생
                              │
                              ▼
                         EventBus
                              │
               ┌──────────────┼──────────────┐
               ▼              ▼              ▼
        LoggingHandler  SyncHandler  PersistenceHandler
               │              │              │
               ▼              ▼              ▼
           File Log    SessionManager   Repository
```

---

## 11. 레퍼런스

### 11.1 현재 관련 소스 파일

| 경로 | 라인 | 설명 |
|------|------|------|
| `domain/src/event.rs` | 1-47 | `DomainEvent` enum 정의 |
| `domain/src/character.rs` | 123-139 | `gain_experience()` — 이벤트 생성 |
| `domain/src/combat.rs` | 61-102 | `process_attack()` — 이벤트 생성 |
| `core/scheduler/src/lib.rs` | 1-138 | `Scheduler` — 타이머 기반 이벤트 예약 |
| `core/security/src/lib.rs` | 37 | `EmitEvent` 권한 정의 |

### 11.2 참고 자료

- tokio broadcast: https://docs.rs/tokio/latest/tokio/sync/broadcast/
- Redis Pub/Sub: https://docs.rs/redis/latest/redis/
- Event Sourcing 패턴: Martin Fowler - Event Sourcing
