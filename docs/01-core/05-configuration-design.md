# 05. 설정 시스템 설계 (미구현)

## 1. 개요

The Protocol의 설정 시스템은 아직 구현되지 않았다. 현재 모든 설정값은 코드에서
하드코딩되어 있다. 이 문서는 TOML 기반 설정 파일 구조, 환경 변수 오버라이드,
설정 검증, 핫 리로드, 12-Factor App 원칙 적용 등을 상세히 설계한다.

## 2. 현재 하드코딩 값

### 2.1 코드 내 하드코딩 현황

| 위치 | 값 | 설명 |
|------|-----|------|
| `core/main.rs:39` | `"127.0.0.1:7770"` | TCP 기본 바인드 주소 |
| `core/main.rs:42` | `"./plugins"` | 플러그인 디렉토리 |
| `core/main.rs:71` | `1000` | 최대 동시 연결 수 |
| `core/protocol/src/message.rs:3` | `PROTOCOL_VERSION = 1` | 프로토콜 버전 |
| `core/protocol/src/message.rs:77` | `30000` | 하트비트 간격 (ms) |
| `core/observability/src/lib.rs:5` | `"info"` | 기본 로깅 레벨 |

### 2.2 CLI 인자 (현재 구현)

```rust
#[derive(Parser)]
#[command(name = "runtime", about = "The Protocol - Cross-Platform Game Runtime")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Server {
        #[arg(short, long, default_value = "127.0.0.1:7770")]
        bind: String,
        #[arg(short, long, default_value = "./plugins")]
        plugins: String,
    },
    Client {
        #[arg(short, long, default_value = "127.0.0.1:7770")]
        server: String,
    },
    Gateway {
        #[arg(short, long, default_value = "127.0.0.1:7770")]
        bind: String,
    },
}
```

**제한점**: CLI 인자로만 설정 가능, 파일 기반 설정 없음

## 3. TOML 설정 파일 구조

### 3.1 파일 경로 우선순위

```
1. CLI 인자 (--config <path>)
2. 환경 변수 (TP_CONFIG_PATH)
3. 현재 디렉토리 (./the-protocol.toml)
4. 사용자 홈 (~/.config/the-protocol/config.toml)
5. 시스템 기본값 (compile-time 기본값)
```

### 3.2 전체 설정 구조체

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,         // 서버 설정
    pub network: NetworkConfig,       // 네트워크 설정
    pub session: SessionConfig,       // 세션 설정
    pub game: GameConfig,             // 게임 로직 설정
    pub plugin: PluginConfig,         // 플러그인 설정
    pub observability: ObservabilityConfig, // 로깅/모니터링
    pub security: SecurityConfig,     // 보안 설정
}
```

### 3.3 ServerConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// 서버 모드 (server, client, gateway)
    pub mode: String,                          // default: "server"
    /// 서버 이름 (식별용)
    pub name: String,                          // default: "The Protocol Server"
    /// 서버 버전
    pub version: String,                       // default: env!("CARGO_PKG_VERSION")
}
```

### 3.4 NetworkConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// TCP 바인드 주소
    pub tcp_bind: String,                      // default: "127.0.0.1:7770"
    /// UDP 바인드 주소 (미구현)
    pub udp_bind: Option<String>,              // default: None
    /// HTTP 서버 바인드 주소 (미구현)
    pub http_bind: Option<String>,             // default: None
    /// WebSocket 바인드 주소 (미구현)
    pub websocket_bind: Option<String>,        // default: None
    /// TCP_NODELAY 활성화
    pub tcp_nodelay: bool,                     // default: true
    /// 소켓 버퍼 크기
    pub socket_buffer_size: usize,             // default: 65536
    /// TCP backlog
    pub tcp_backlog: u32,                      // default: 128
}
```

### 3.5 SessionConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// 최대 동시 연결 수
    pub max_connections: usize,                // default: 1000
    /// 하트비트 간격 (밀리초)
    pub heartbeat_interval_ms: u64,            // default: 30000
    /// 하트비트 타임아웃 승수
    pub heartbeat_timeout_multiplier: f64,     // default: 2.5
    /// 최대 세션 지속 시간 (초)
    pub max_session_duration_secs: u64,        // default: 3600
    /// mpsc 채널 버퍼 크기
    pub channel_buffer_size: usize,            // default: 256
    /// 세션 ID 시드
    pub session_id_seed: Option<u64>,          // default: None (random)
}
```

### 3.6 GameConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    /// 시작 방 ID
    pub starting_room_id: u32,                 // default: 1
    /// 최대 캐릭터 수 (세션당)
    pub max_characters_per_session: u32,       // default: 1
    /// 기본 레벨업 경험치
    pub base_xp_per_level: u64,               // default: 1000
    /// 기본 HP
    pub base_hp: u32,                          // default: 50
    /// 전투 기본 데미지 변동폭
    pub combat_damage_variance: f64,           // default: 0.2
}
```

### 3.7 PluginConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    /// 플러그인 디렉토리 경로
    pub directory: String,                     // default: "./plugins"
    /// 플러그인 자동 로드
    pub auto_load: bool,                       // default: true
    /// 플러그인 최대 메모리 (바이트)
    pub max_memory_per_plugin: usize,          // default: 67108864 (64MB)
    /// 플러그인 최대 실행 시간 (밀리초)
    pub max_execution_time_ms: u64,            // default: 100
}
```

### 3.8 ObservabilityConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    /// 로깅 레벨 (trace, debug, info, warn, error)
    pub log_level: String,                     // default: "info"
    /// 로그 포맷 (text, json)
    pub log_format: String,                    // default: "text"
    /// 스레드 ID 포함 여부
    pub log_thread_ids: bool,                  // default: true
    /// 타겟 포함 여부
    pub log_with_target: bool,                 // default: true
    /// 로그 파일 경로 (None = stdout)
    pub log_file: Option<String>,              // default: None
    /// 메트릭 활성화
    pub metrics_enabled: bool,                 // default: false
    /// 메트릭 바인드 주소
    pub metrics_bind: String,                  // default: "127.0.0.1:9090"
}
```

### 3.9 SecurityConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// 인증 필수 여부
    pub auth_required: bool,                   // default: false
    /// JWT 시크릿 (인증 시)
    pub jwt_secret: Option<String>,            // default: None
    /// JWT 만료 시간 (초)
    pub jwt_expiry_secs: u64,                  // default: 3600
    /// Rate Limiting 활성화
    pub rate_limit_enabled: bool,              // default: false
    /// Rate Limit: 초당 최대 요청 수
    pub rate_limit_per_second: u32,            // default: 10
    /// Rate Limit: 버킷 크기
    pub rate_limit_bucket_size: u32,           // default: 50
}
```

## 4. 환경 변수 오버라이드

### 4.1 매핑 규칙

| 환경 변수 | 설정 경로 | 예시 |
|-----------|-----------|------|
| `TP_SERVER_MODE` | `server.mode` | `TP_SERVER_MODE=client` |
| `TP_TCP_BIND` | `network.tcp_bind` | `TP_TCP_BIND=0.0.0.0:7770` |
| `TP_MAX_CONNECTIONS` | `session.max_connections` | `TP_MAX_CONNECTIONS=5000` |
| `TP_HEARTBEAT_MS` | `session.heartbeat_interval_ms` | `TP_HEARTBEAT_MS=15000` |
| `TP_LOG_LEVEL` | `observability.log_level` | `TP_LOG_LEVEL=debug` |
| `TP_LOG_FORMAT` | `observability.log_format` | `TP_LOG_FORMAT=json` |
| `TP_PLUGIN_DIR` | `plugin.directory` | `TP_PLUGIN_DIR=/opt/plugins` |
| `TP_AUTH_REQUIRED` | `security.auth_required` | `TP_AUTH_REQUIRED=true` |
| `TP_JWT_SECRET` | `security.jwt_secret` | `TP_JWT_SECRET=my-secret` |
| `TP_CONFIG_PATH` | (설정 파일 경로) | `TP_CONFIG_PATH=./custom.toml` |

### 4.2 구현 방안

```rust
use std::env;

impl AppConfig {
    pub fn from_env(mut self) -> Self {
        // 서버 모드
        if let Ok(mode) = env::var("TP_SERVER_MODE") {
            self.server.mode = mode;
        }

        // TCP 바인드
        if let Ok(bind) = env::var("TP_TCP_BIND") {
            self.network.tcp_bind = bind;
        }

        // 최대 연결 수
        if let Ok(max) = env::var("TP_MAX_CONNECTIONS") {
            if let Ok(n) = max.parse() {
                self.session.max_connections = n;
            }
        }

        // 로깅 레벨
        if let Ok(level) = env::var("TP_LOG_LEVEL") {
            self.observability.log_level = level;
        }

        self
    }
}
```

### 4.3 우선순위

```
1. CLI 인자 (최고 우선)
2. 환경 변수
3. 설정 파일
4. 기본값 (최저 우선)
```

## 5. 설정 검증 로직

### 5.1 검증 규칙

```rust
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Invalid bind address: {0}")]
    InvalidBindAddress(String),

    #[error("Max connections must be > 0, got {0}")]
    InvalidMaxConnections(usize),

    #[error("Heartbeat interval must be >= 5000ms, got {0}")]
    InvalidHeartbeatInterval(u64),

    #[error("Log level must be one of: trace, debug, info, warn, error")]
    InvalidLogLevel,

    #[error("Config file not found: {0}")]
    FileNotFound(String),

    #[error("TOML parse error: {0}")]
    TomlParseError(String),

    #[error("Missing required field: {0}")]
    MissingField(String),
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        // 1. 바인드 주소 검증
        self.network.tcp_bind.parse::<std::net::SocketAddr>()
            .map_err(|_| ConfigError::InvalidBindAddress(self.network.tcp_bind.clone()))?;

        // 2. 최대 연결 수
        if self.session.max_connections == 0 {
            return Err(ConfigError::InvalidMaxConnections(0));
        }

        // 3. 하트비트 간격
        if self.session.heartbeat_interval_ms < 5000 {
            return Err(ConfigError::InvalidHeartbeatInterval(
                self.session.heartbeat_interval_ms
            ));
        }

        // 4. 로깅 레벨
        match self.observability.log_level.as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => {}
            _ => return Err(ConfigError::InvalidLogLevel),
        }

        // 5. 하트비트 타임아웃 승수
        if self.session.heartbeat_timeout_multiplier < 1.0 {
            return Err(ConfigError::InvalidHeartbeatInterval(0));
        }

        Ok(())
    }
}
```

### 5.2 검증 시점

```
[시작] → 설정 로드 → 설정 검증 → 검증 실패 시 프로그램 종료
                                    │
                                    ├─ 에러 메시지 출력
                                    └─ exit(1)

[핫 리로드] → 설정 재로드 → 검증 → 검증 실패 시 이전 설정 유지 + 경고 로그
```

## 6. 핫 리로드 가능 항목

### 6.1 핫 리로드 가능 (런타임에 변경 가능)

| 항목 | 변경 방법 | 영향 |
|------|-----------|------|
| `observability.log_level` | 파일 수정 또는 API | 즉시 적용 |
| `session.heartbeat_interval_ms` | 파일 수정 | 새 연결에 적용 |
| `plugin.directory` | 파일 수정 | 다음 플러그인 로드 시 |
| `security.rate_limit_per_second` | 파일 수정 | 즉시 적용 |

### 6.2 핫 리로드 불가능 (재시작 필요)

| 항목 | 변경 방법 | 설명 |
|------|-----------|------|
| `network.tcp_bind` | 재시작 | 소켓 바인드 변경 |
| `session.max_connections` | 재시작 | 리소스 할당 변경 |
| `server.mode` | 재시작 | 전체 아키텍처 변경 |

### 6.3 핫 리로드 구현

```rust
use tokio::sync::watch;

pub struct ConfigReloader {
    config_tx: watch::Sender<AppConfig>,
    config_rx: watch::Receiver<AppConfig>,
    watch_path: PathBuf,
}

impl ConfigReloader {
    pub async fn start_watching(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        let mut last_modified = std::fs::metadata(&self.watch_path)
            .and_then(|m| m.modified())
            .ok();

        loop {
            interval.tick().await;

            let current_modified = std::fs::metadata(&self.watch_path)
                .and_then(|m| m.modified())
                .ok();

            if current_modified != last_modified {
                match self.reload() {
                    Ok(new_config) => {
                        self.config_tx.send(new_config).ok();
                        tracing::info!("Config reloaded");
                    }
                    Err(e) => {
                        tracing::warn!("Config reload failed: {}, keeping old config", e);
                    }
                }
                last_modified = current_modified;
            }
        }
    }

    fn reload(&self) -> Result<AppConfig, ConfigError> {
        let content = std::fs::read_to_string(&self.watch_path)
            .map_err(|_| ConfigError::FileNotFound(
                self.watch_path.display().to_string()
            ))?;
        let config: AppConfig = toml::from_str(&content)
            .map_err(|e| ConfigError::TomlParseError(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }
}
```

## 7. 기본값 전략

### 7.1 Default 트레이트 구현

```rust
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            network: NetworkConfig::default(),
            session: SessionConfig::default(),
            game: GameConfig::default(),
            plugin: PluginConfig::default(),
            observability: ObservabilityConfig::default(),
            security: SecurityConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            mode: "server".to_string(),
            name: "The Protocol Server".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            tcp_bind: "127.0.0.1:7770".to_string(),
            udp_bind: None,
            http_bind: None,
            websocket_bind: None,
            tcp_nodelay: true,
            socket_buffer_size: 65536,
            tcp_backlog: 128,
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_connections: 1000,
            heartbeat_interval_ms: 30000,
            heartbeat_timeout_multiplier: 2.5,
            max_session_duration_secs: 3600,
            channel_buffer_size: 256,
            session_id_seed: None,
        }
    }
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            starting_room_id: 1,
            max_characters_per_session: 1,
            base_xp_per_level: 1000,
            base_hp: 50,
            combat_damage_variance: 0.2,
        }
    }
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            directory: "./plugins".to_string(),
            auto_load: true,
            max_memory_per_plugin: 64 * 1024 * 1024,
            max_execution_time_ms: 100,
        }
    }
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            log_format: "text".to_string(),
            log_thread_ids: true,
            log_with_target: true,
            log_file: None,
            metrics_enabled: false,
            metrics_bind: "127.0.0.1:9090".to_string(),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            auth_required: false,
            jwt_secret: None,
            jwt_expiry_secs: 3600,
            rate_limit_enabled: false,
            rate_limit_per_second: 10,
            rate_limit_bucket_size: 50,
        }
    }
}
```

## 8. 설정 파일 예시

### 8.1 Server 모드 (`the-protocol.server.toml`)

```toml
# The Protocol - Server Mode Configuration

[server]
mode = "server"
name = "Aetherius Game Server"
version = "0.1.0"

[network]
tcp_bind = "0.0.0.0:7770"
tcp_nodelay = true
socket_buffer_size = 65536
tcp_backlog = 256

[session]
max_connections = 2000
heartbeat_interval_ms = 30000
heartbeat_timeout_multiplier = 2.5
max_session_duration_secs = 7200
channel_buffer_size = 512

[game]
starting_room_id = 1
max_characters_per_session = 3
base_xp_per_level = 1000
base_hp = 50
combat_damage_variance = 0.2

[plugin]
directory = "./plugins"
auto_load = true
max_memory_per_plugin = 134217728  # 128MB
max_execution_time_ms = 200

[observability]
log_level = "info"
log_format = "json"
log_thread_ids = true
log_with_target = true
log_file = "./logs/server.log"
metrics_enabled = true
metrics_bind = "0.0.0.0:9090"

[security]
auth_required = true
jwt_secret = "CHANGE_ME_IN_PRODUCTION"
jwt_expiry_secs = 3600
rate_limit_enabled = true
rate_limit_per_second = 20
rate_limit_bucket_size = 100
```

### 8.2 Client 모드 (`the-protocol.client.toml`)

```toml
# The Protocol - Client Mode Configuration

[server]
mode = "client"
name = "MUD Client"
version = "0.1.0"

[network]
tcp_bind = "127.0.0.1:7770"  # 서버 주소 (클라이언트 모드에서는 대상 주소)
tcp_nodelay = true

[session]
max_connections = 1           # 클라이언트는 1개 연결만
heartbeat_interval_ms = 30000

[game]
starting_room_id = 1

[plugin]
directory = "./client-plugins"
auto_load = false

[observability]
log_level = "warn"
log_format = "text"
log_with_target = false

[security]
auth_required = false
```

### 8.3 Gateway 모드 (`the-protocol.gateway.toml`)

```toml
# The Protocol - Gateway Mode Configuration

[server]
mode = "gateway"
name = "Aetherius Gateway"
version = "0.1.0"

[network]
tcp_bind = "0.0.0.0:7770"
http_bind = "0.0.0.0:7771"
websocket_bind = "0.0.0.0:7772"
tcp_nodelay = true
socket_buffer_size = 131072   # 128KB (게이트웨이는 더 큰 버퍼)
tcp_backlog = 512

[session]
max_connections = 5000
heartbeat_interval_ms = 15000  # 게이트웨이는 더 짧은 하트비트
heartbeat_timeout_multiplier = 2.0
max_session_duration_secs = 86400  # 24시간

[plugin]
directory = "./gateway-plugins"
auto_load = true

[observability]
log_level = "info"
log_format = "json"
log_thread_ids = true
metrics_enabled = true
metrics_bind = "0.0.0.0:9091"

[security]
auth_required = true
jwt_secret = "GATEWAY_SECRET_KEY"
rate_limit_enabled = true
rate_limit_per_second = 50
rate_limit_bucket_size = 200
```

## 9. 12-Factor App 원칙 적용

### 9.1 원칙별 적용 방안

| # | 원칙 | 적용 방법 |
|---|------|-----------|
| 1 | **Codebase** | 설정 파일은 Git에 포함, 비밀값은 제외 |
| 2 | **Dependencies** | `Cargo.toml` 명시적 의존성 관리 |
| 3 | **Config** | 환경 변수로 환경별 설정 분리 |
| 4 | **Backing Services** | DB, Redis 등은 설정으로 연결 |
| 5 | **Build/Run/Ship** | 동일 바이너리로 환경별 실행 |
| 6 | **Processes** | 설정은 한 번 로드, 런타임 불변 (핫 리로드 선택적) |
| 7 | **Port Binding** | 설정 가능한 바인드 주소 |
| 8 | **Concurrency** | 설정 기반 워커 수 조정 |
| 9 | **Disposability** | Graceful shutdown 시 설정 리소스 정리 |
| 10 | **Dev/Prod Parity** | 동일 설정 구조, 값만 다름 |
| 11 | **Logs** | 로깅 설정은 환경 변수로 제어 |
| 12 | **Admin Processes** | 관리자 커맨드는 별도 모드로 실행 |

### 9.2 환경 변수 기반 설정 분리

```bash
# 개발 환경
export TP_LOG_LEVEL=debug
export TP_LOG_FORMAT=text
export TP_TCP_BIND=127.0.0.1:7770
export TP_AUTH_REQUIRED=false

# 스테이징 환경
export TP_LOG_LEVEL=info
export TP_LOG_FORMAT=json
export TP_TCP_BIND=0.0.0.0:7770
export TP_AUTH_REQUIRED=true
export TP_JWT_SECRET=staging-secret-key

# 프로덕션 환경
export TP_LOG_LEVEL=warn
export TP_LOG_FORMAT=json
export TP_TCP_BIND=0.0.0.0:7770
export TP_MAX_CONNECTIONS=10000
export TP_AUTH_REQUIRED=true
export TP_JWT_SECRET=production-secret-key
export TP_RATE_LIMIT_ENABLED=true
export TP_RATE_LIMIT_PER_SECOND=50
```

## 10. 설정 로드 시퀀스

```
[프로그램 시작]
    │
    ▼
[1. 기본값 로드]
    AppConfig::default()
    │
    ▼
[2. 설정 파일 로드]
    TP_CONFIG_PATH 환경 변수 확인
    │
    ├─ 있음 → 해당 파일 로드
    │
    ├─ 없음 → 기본 경로 탐색
    │   ./the-protocol.toml
    │   ~/.config/the-protocol/config.toml
    │
    ├─ 파일 없음 → 기본값 유지 (경고 로그)
    │
    ▼
[3. TOML 파싱]
    toml::from_str::<AppConfig>(content)
    │
    ├─ 실패 → 에러 출력 후 종료
    │
    ▼
[4. 환경 변수 오버라이드]
    config.from_env()
    │
    ▼
[5. CLI 인자 오버라이드]
    Cli::parse() → 설정값 덮어쓰기
    │
    ▼
[6. 설정 검증]
    config.validate()
    │
    ├─ 실패 → 에러 출력 후 종료
    │
    ▼
[7. 로깅 초기화]
    protocol_observability::init_logging() with config
    │
    ▼
[8. 설정 구조체 전달]
    Arc::new(config) → 각 모듈에 전달
```

## 11. 설정 구조체 → 기존 코드 매핑

### 11.1 현재 하드코딩 → 설정 필드

| 현재 코드 | 설정 필드 | 파일 |
|-----------|-----------|------|
| `"127.0.0.1:7770"` (CLI 기본값) | `network.tcp_bind` | `core/main.rs:39` |
| `"./plugins"` (CLI 기본값) | `plugin.directory` | `core/main.rs:42` |
| `SessionManager::new(1000)` | `session.max_connections` | `core/main.rs:71` |
| `PROTOCOL_VERSION = 1` | (상수 유지) | `core/protocol/src/message.rs:3` |
| `heartbeat_interval_ms: 30000` | `session.heartbeat_interval_ms` | `core/protocol/src/message.rs:77` |
| `channel(256)` | `session.channel_buffer_size` | `core/session/src/lib.rs:76` |

### 11.2 리팩토링 예시

```rust
// 현재 (하드코딩)
async fn run_server(bind: &str, plugin_dir: &str) -> Result<()> {
    let session_manager = Arc::new(SessionManager::new(1000));
    let _network = NetworkManager::new(bind, session_manager.clone()).await?;
    let mut plugin_runtime = DefaultPluginRuntime::new(plugin_dir);
    // ...
}

// 개선 (설정 기반)
async fn run_server(config: AppConfig) -> Result<()> {
    let session_manager = Arc::new(SessionManager::new(
        config.session.max_connections,
        config.session.channel_buffer_size,
    ));
    let network = NetworkManager::new(
        &config.network.tcp_bind,
        session_manager.clone(),
    ).await?;
    let mut plugin_runtime = DefaultPluginRuntime::new(&config.plugin.directory);
    // ...
}
```

## 12. 요약

| 구성 요소 | 현재 상태 | 설계 상태 |
|-----------|-----------|-----------|
| TOML 설정 파일 | ❌ 미구현 | ✅ 7개 섹션 구조 완성 |
| 환경 변수 오버라이드 | ❌ 미구현 | ✅ 10개 변수 매핑 |
| 설정 검증 | ❌ 미구현 | ✅ 5개 검증 규칙 |
| 핫 리로드 | ❌ 미구현 | ✅ watch 기반 설계 |
| 기본값 | ⚠️ 하드코딩 | ✅ Default 구현 설계 |
| 12-Factor App | ❌ 미적용 | ✅ 원칙별 매핑 |

설정 시스템은 The Protocol의 프로덕션 배포를 위한 핵심 인프라이다.
가장 먼저 구현해야 할 항목은 **TOML 설정 파일 로드 + 환경 변수 오버라이드**이며,
그 다음으로 **설정 검증 + 핫 리로드**를 구현하면 된다.
