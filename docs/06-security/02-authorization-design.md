# 06-02 - 권한 체계 및 Capability 시스템 설계

## 개요

The Protocol은 이중 권한 체계를 사용한다: **Capability System** (플러그인/런타임 수준)과 **RBAC** (플레이어 수준). 이 시스템은 WASM 샌드박스와 결합하여 보안을 보장한다.

## 권한 체계 (19개 Permission)

현재 `core/security/src/lib.rs`에 정의된 권한 목록:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    // Player
    PlayerRead,          // "player.read"
    PlayerModify,        // "player.modify"

    // Inventory
    InventoryRead,       // "inventory.read"
    InventoryModify,     // "inventory.modify"

    // Combat
    CombatRead,          // "combat.read"
    CombatModify,        // "combat.modify"

    // World
    WorldRead,           // "world.read"
    WorldModify,         // "world.modify"

    // Auction
    AuctionRead,         // "auction.read"
    AuctionModify,       // "auction.modify"

    // Guild
    GuildRead,           // "guild.read"
    GuildModify,         // "guild.modify"

    // Infrastructure
    DatabaseRead,        // "database.read"
    DatabaseWrite,       // "database.write"
    CacheRead,           // "cache.read"
    CacheWrite,          // "cache.write"

    // System
    ScheduleTimer,       // "schedule.timer"
    EmitEvent,           // "emit.event"
    RegisterCommand,     // "register.command"

    // Custom
    Custom(String),      // 커스텀 권한
}
```

### 권한 카테고리

| 카테고리 | 권한 | 읽기 | 수정 | 설명 |
|---------|------|------|------|------|
| Player | player | `PlayerRead` | `PlayerModify` | 캐릭터 정보 |
| Inventory | inventory | `InventoryRead` | `InventoryModify` | 인벤토리 관리 |
| Combat | combat | `CombatRead` | `CombatModify` | 전투 시스템 |
| World | world | `WorldRead` | `WorldModify` | 월드 상태 |
| Auction | auction | `AuctionRead` | `AuctionModify` | 경매 시스템 |
| Guild | guild | `GuildRead` | `GuildModify` | 길드 시스템 |
| Infrastructure | database | `DatabaseRead` | `DatabaseWrite` | DB 접근 |
| Infrastructure | cache | `CacheRead` | `CacheWrite` | 캐시 접근 |
| System | schedule | `ScheduleTimer` | - | 타이머 등록 |
| System | event | `EmitEvent` | - | 이벤트 발행 |
| System | command | `RegisterCommand` | - | 커맨드 등록 |

### 권한 매핑

```rust
impl Permission {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "player.read" => Some(Self::PlayerRead),
            "player.modify" => Some(Self::PlayerModify),
            "inventory.read" => Some(Self::InventoryRead),
            "inventory.modify" => Some(Self::InventoryModify),
            "combat.read" => Some(Self::CombatRead),
            "combat.modify" => Some(Self::CombatModify),
            "world.read" => Some(Self::WorldRead),
            "world.modify" => Some(Self::WorldModify),
            "auction.read" => Some(Self::AuctionRead),
            "auction.modify" => Some(Self::AuctionModify),
            "guild.read" => Some(Self::GuildRead),
            "guild.modify" => Some(Self::GuildModify),
            "database.read" => Some(Self::DatabaseRead),
            "database.write" => Some(Self::DatabaseWrite),
            "cache.read" => Some(Self::CacheRead),
            "cache.write" => Some(Self::CacheWrite),
            "schedule.timer" => Some(Self::ScheduleTimer),
            "emit.event" => Some(Self::EmitEvent),
            "register.command" => Some(Self::RegisterCommand),
            other => Some(Self::Custom(other.to_string())),
        }
    }

    pub fn to_str(&self) -> &str {
        match self {
            Self::PlayerRead => "player.read",
            Self::PlayerModify => "player.modify",
            Self::InventoryRead => "inventory.read",
            Self::InventoryModify => "inventory.modify",
            Self::CombatRead => "combat.read",
            Self::CombatModify => "combat.modify",
            Self::WorldRead => "world.read",
            Self::WorldModify => "world.modify",
            Self::AuctionRead => "auction.read",
            Self::AuctionModify => "auction.modify",
            Self::GuildRead => "guild.read",
            Self::GuildModify => "guild.modify",
            Self::DatabaseRead => "database.read",
            Self::DatabaseWrite => "database.write",
            Self::CacheRead => "cache.read",
            Self::CacheWrite => "cache.write",
            Self::ScheduleTimer => "schedule.timer",
            Self::EmitEvent => "emit.event",
            Self::RegisterCommand => "register.command",
            Self::Custom(s) => s,
        }
    }
}
```

## Capability System

### Runtime Capability vs Plugin Capability

```rust
/// 런타임 수준 Capability (서버 프로세스의 리소스 접근 권한)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    pub tcp_listener: bool,
    pub udp_listener: bool,
    pub tcp_client: bool,
    pub udp_client: bool,
    pub http_server: bool,
    pub http_client: bool,
    pub websocket_server: bool,
    pub websocket_client: bool,
    pub plugin_runtime: bool,
    pub session_manager: bool,
    pub scheduler: bool,
    pub event_bus: bool,
    pub database: bool,
    pub cache: bool,
    pub metrics: bool,
}

/// 플러그인 수준 Capability (WASM 플러그인의 접근 권한)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCapabilities {
    /// 허용된 권한 목록
    pub permissions: Vec<Permission>,
    /// 메모리 제한 (바이트)
    pub memory_limit: Option<usize>,
    /// 실행 시간 제한 (밀리초)
    pub execution_limit_ms: Option<u64>,
    /// 저장소 접근 권한
    pub storage_access: bool,
}
```

### Runtime Capability 프로필

```rust
impl RuntimeCapabilities {
    /// 전체 서버 프로필
    pub fn server() -> Self {
        Self {
            tcp_listener: true,
            udp_listener: true,
            tcp_client: false,
            udp_client: false,
            http_server: true,
            http_client: false,
            websocket_server: true,
            websocket_client: false,
            plugin_runtime: true,
            session_manager: true,
            scheduler: true,
            event_bus: true,
            database: true,
            cache: true,
            metrics: true,
        }
    }

    /// 클라이언트 프로필
    pub fn client() -> Self {
        Self {
            tcp_listener: false,
            udp_listener: false,
            tcp_client: true,
            udp_client: true,
            http_server: false,
            http_client: true,
            websocket_server: false,
            websocket_client: true,
            plugin_runtime: true,
            session_manager: true,
            scheduler: false,
            event_bus: true,
            database: false,
            cache: false,
            metrics: false,
        }
    }

    /// 게이트웨이 프로필
    pub fn gateway() -> Self {
        Self {
            tcp_listener: true,
            udp_listener: true,
            tcp_client: true,
            udp_client: true,
            http_server: true,
            http_client: false,
            websocket_server: true,
            websocket_client: false,
            plugin_runtime: true,
            session_manager: true,
            scheduler: true,
            event_bus: true,
            database: false,
            cache: true,
            metrics: true,
        }
    }
}
```

### Capability Manager

```rust
pub struct CapabilityManager {
    runtime_capabilities: RuntimeCapabilities,
    plugin_capabilities: DashMap<String, PluginCapabilities>,
}

impl CapabilityManager {
    pub fn new(runtime_caps: RuntimeCapabilities) -> Self {
        Self {
            runtime_capabilities: runtime_caps,
            plugin_capabilities: DashMap::new(),
        }
    }

    /// 플러그인 등록
    pub fn register_plugin(&self, name: &str, caps: PluginCapabilities) {
        self.plugin_capabilities.insert(name.to_string(), caps);
        tracing::info!(
            plugin = name,
            permissions = ?caps.permissions.iter().map(|p| p.to_str()).collect::<Vec<_>>(),
            "Plugin capabilities registered"
        );
    }

    /// 플러그인 권한 검증
    pub fn check_permission(&self, plugin: &str, permission: &Permission) -> bool {
        if let Some(caps) = self.plugin_capabilities.get(plugin) {
            caps.permissions.contains(permission)
        } else {
            false
        }
    }

    /// 런타임 Capability 확인
    pub fn has_runtime_capability(&self, cap: &str) -> bool {
        match cap {
            "tcp_listener" => self.runtime_capabilities.tcp_listener,
            "udp_listener" => self.runtime_capabilities.udp_listener,
            "tcp_client" => self.runtime_capabilities.tcp_client,
            "udp_client" => self.runtime_capabilities.udp_client,
            "http_server" => self.runtime_capabilities.http_server,
            "http_client" => self.runtime_capabilities.http_client,
            "websocket_server" => self.runtime_capabilities.websocket_server,
            "websocket_client" => self.runtime_capabilities.websocket_client,
            "plugin_runtime" => self.runtime_capabilities.plugin_runtime,
            "session_manager" => self.runtime_capabilities.session_manager,
            "scheduler" => self.runtime_capabilities.scheduler,
            "event_bus" => self.runtime_capabilities.event_bus,
            "database" => self.runtime_capabilities.database,
            "cache" => self.runtime_capabilities.cache,
            "metrics" => self.runtime_capabilities.metrics,
            _ => false,
        }
    }

    /// 플러그인 권한 일괄 검증
    pub fn check_all_permissions(
        &self,
        plugin: &str,
        required: &[Permission],
    ) -> Result<(), Vec<Permission>> {
        let missing: Vec<Permission> = required.iter()
            .filter(|p| !self.check_permission(plugin, p))
            .cloned()
            .collect();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }
}
```

## 역할 기반 접근 제어 (RBAC)

### 역할 정의

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Player,
    Moderator,
    Admin,
    System,
}

impl Role {
    pub fn permissions(&self) -> Vec<Permission> {
        match self {
            Role::Player => vec![
                Permission::PlayerRead,
                Permission::PlayerModify,
                Permission::InventoryRead,
                Permission::InventoryModify,
                Permission::CombatRead,
                Permission::CombatModify,
                Permission::WorldRead,
            ],
            Role::Moderator => vec![
                // Player 권한 포함
                Permission::PlayerRead,
                Permission::PlayerModify,
                Permission::InventoryRead,
                Permission::InventoryModify,
                Permission::CombatRead,
                Permission::CombatModify,
                Permission::WorldRead,
                Permission::WorldModify,
                // 추가 권한
                Permission::AuctionRead,
                Permission::AuctionModify,
                Permission::GuildRead,
                Permission::GuildModify,
            ],
            Role::Admin => vec![
                // 모든 권한
                Permission::PlayerRead,
                Permission::PlayerModify,
                Permission::InventoryRead,
                Permission::InventoryModify,
                Permission::CombatRead,
                Permission::CombatModify,
                Permission::WorldRead,
                Permission::WorldModify,
                Permission::AuctionRead,
                Permission::AuctionModify,
                Permission::GuildRead,
                Permission::GuildModify,
                Permission::DatabaseRead,
                Permission::DatabaseWrite,
                Permission::CacheRead,
                Permission::CacheWrite,
                Permission::ScheduleTimer,
                Permission::EmitEvent,
                Permission::RegisterCommand,
            ],
            Role::System => vec![
                // 모든 권한 + 시스템 전용
                Permission::PlayerRead,
                Permission::PlayerModify,
                Permission::InventoryRead,
                Permission::InventoryModify,
                Permission::CombatRead,
                Permission::CombatModify,
                Permission::WorldRead,
                Permission::WorldModify,
                Permission::AuctionRead,
                Permission::AuctionModify,
                Permission::GuildRead,
                Permission::GuildModify,
                Permission::DatabaseRead,
                Permission::DatabaseWrite,
                Permission::CacheRead,
                Permission::CacheWrite,
                Permission::ScheduleTimer,
                Permission::EmitEvent,
                Permission::RegisterCommand,
            ],
        }
    }

    pub fn level(&self) -> u32 {
        match self {
            Role::Player => 0,
            Role::Moderator => 1,
            Role::Admin => 2,
            Role::System => 3,
        }
    }
}
```

### 역할 관리자

```rust
pub struct RoleManager {
    /// 플레이어별 역할 매핑
    player_roles: DashMap<u64, Vec<Role>>,
    /// 역할별 커스텀 권한 (기본 권한에 추가)
    custom_permissions: DashMap<Role, Vec<Permission>>,
}

impl RoleManager {
    pub fn new() -> Self {
        Self {
            player_roles: DashMap::new(),
            custom_permissions: DashMap::new(),
        }
    }

    /// 플레이어 역할 할당
    pub fn assign_role(&self, player_id: u64, role: Role) {
        self.player_roles
            .entry(player_id)
            .or_insert_with(Vec::new)
            .push(role);
    }

    /// 플레이어 역할 제거
    pub fn remove_role(&self, player_id: u64, role: &Role) {
        if let Some(mut roles) = self.player_roles.get_mut(&player_id) {
            roles.retain(|r| r != role);
        }
    }

    /// 플레이어의 전체 권한 목록 (역할 기본 + 커스텀)
    pub fn get_permissions(&self, player_id: u64) -> Vec<Permission> {
        let mut permissions = Vec::new();

        if let Some(roles) = self.player_roles.get(&player_id) {
            for role in roles.iter() {
                permissions.extend(role.permissions());

                // 커스텀 권한 추가
                if let Some(custom) = self.custom_permissions.get(role) {
                    permissions.extend(custom.iter().cloned());
                }
            }
        }

        permissions.sort_by_key(|p| p.to_str().to_string());
        permissions.dedup();
        permissions
    }

    /// 권한 검증
    pub fn has_permission(&self, player_id: u64, permission: &Permission) -> bool {
        self.get_permissions(player_id).contains(permission)
    }

    /// 역할 레벨 확인
    pub fn max_role_level(&self, player_id: u64) -> u32 {
        self.player_roles
            .get(&player_id)
            .map(|roles| roles.iter().map(|r| r.level()).max().unwrap_or(0))
            .unwrap_or(0)
    }
}
```

## 정책 기반 접근 제어

```rust
pub struct PolicyEngine {
    policies: Vec<Box<dyn Policy>>,
}

pub enum PolicyResult {
    Allow,
    Deny(String),
}

pub trait Policy: Send + Sync {
    fn evaluate(&self, ctx: &PolicyContext) -> PolicyResult;
    fn name(&self) -> &str;
}

pub struct PolicyContext {
    pub player_id: u64,
    pub role: Role,
    pub permissions: Vec<Permission>,
    pub action: String,
    pub resource: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// 예시 정책: 플레이어는 자기 캐릭터만 수정 가능
pub struct OwnershipPolicy;

impl Policy for OwnershipPolicy {
    fn evaluate(&self, ctx: &PolicyContext) -> PolicyResult {
        if ctx.action == "modify" && ctx.resource.starts_with("character:") {
            let target_id: u64 = ctx.resource.trim_start_matches("character:")
                .parse()
                .unwrap_or(0);

            if ctx.player_id != target_id && ctx.role.level() < Role::Admin.level() {
                return PolicyResult::Deny(
                    "You can only modify your own character".to_string()
                );
            }
        }
        PolicyResult::Allow
    }

    fn name(&self) -> &str {
        "ownership"
    }
}

// 예시 정책: 리소스당 시간 기반 제한
pub struct CooldownPolicy {
    cooldowns: DashMap<String, std::time::Instant>,
    cooldown_duration: std::time::Duration,
}

impl Policy for CooldownPolicy {
    fn evaluate(&self, ctx: &PolicyContext) -> PolicyResult {
        let key = format!("{}:{}", ctx.player_id, ctx.action);

        if let Some(last_used) = self.cooldowns.get(&key) {
            if last_used.elapsed() < self.cooldown_duration {
                return PolicyResult::Deny(
                    format!("Action '{}' is on cooldown", ctx.action)
                );
            }
        }

        self.cooldowns.insert(key, std::time::Instant::now());
        PolicyResult::Allow
    }

    fn name(&self) -> &str {
        "cooldown"
    }
}
```

## WASM 샌드박스 + Capability 결합

WASM 플러그인은 샌드박스 환경에서 실행되며, Capability 시스템이 제한된 리소스 접근을 보장한다.

```rust
pub struct WasmSandbox {
    capability_manager: Arc<CapabilityManager>,
    plugin_name: String,
}

impl WasmSandbox {
    /// Host Function 호출 시 Capability 검증
    pub fn check_host_function(
        &self,
        function_name: &str,
    ) -> Result<(), PluginError> {
        let required_permission = match function_name {
            "player_get" => Some(Permission::PlayerRead),
            "player_update" => Some(Permission::PlayerModify),
            "inventory_get" => Some(Permission::InventoryRead),
            "inventory_modify" => Some(Permission::InventoryModify),
            "combat_start" => Some(Permission::CombatModify),
            "world_get" => Some(Permission::WorldRead),
            "world_modify" => Some(Permission::WorldModify),
            "storage_get" | "storage_set" => None, // storage_access로 검증
            "emit_event" => Some(Permission::EmitEvent),
            "schedule_timer" => Some(Permission::ScheduleTimer),
            "register_command" => Some(Permission::RegisterCommand),
            _ => None,
        };

        if let Some(permission) = required_permission {
            if !self.capability_manager.check_permission(&self.plugin_name, &permission) {
                return Err(PluginError::PermissionDenied {
                    plugin: self.plugin_name.clone(),
                    permission: permission.to_str().to_string(),
                });
            }
        }

        Ok(())
    }

    /// 리소스 제한 검증
    pub fn check_resource_limits(
        &self,
        memory_used: usize,
        execution_time_ms: u64,
    ) -> Result<(), PluginError> {
        if let Some(caps) = self.capability_manager.plugin_capabilities.get(&self.plugin_name) {
            if let Some(memory_limit) = caps.memory_limit {
                if memory_used > memory_limit {
                    return Err(PluginError::Wasm(format!(
                        "Memory limit exceeded: {} > {}",
                        memory_used, memory_limit
                    )));
                }
            }

            if let Some(exec_limit) = caps.execution_limit_ms {
                if execution_time_ms > exec_limit {
                    return Err(PluginError::Wasm(format!(
                        "Execution time limit exceeded: {}ms > {}ms",
                        execution_time_ms, exec_limit
                    )));
                }
            }
        }

        Ok(())
    }
}
```

### 플러그인 권한 설정 예시 (plugin.toml)

```toml
[permissions]
required = ["player.read", "player.modify"]
optional = ["inventory.read", "combat.read"]

[resources]
memory_limit = "64MB"
execution_limit = "100ms"
storage_access = true
```

## 권한 검증 플로우

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  요청 도착   │────▶│ AuthMiddleware│────▶│ RoleManager  │
└──────────────┘     └──────┬───────┘     └──────┬───────┘
                            │                     │
                     ┌──────▼───────┐      ┌──────▼───────┐
                     │ JWT 검증     │      │ 역할 확인    │
                     │ Claims 추출  │      │ 권한 목록    │
                     └──────┬───────┘      └──────┬───────┘
                            │                     │
                     ┌──────▼─────────────────────▼───────┐
                     │         PolicyEngine               │
                     │  1. OwnershipPolicy                │
                     │  2. CooldownPolicy                 │
                     │  3. 커스텀 정책                     │
                     └──────────────┬─────────────────────┘
                                    │
                     ┌──────────────▼─────────────────────┐
                     │         커맨드 핸들러 실행           │
                     │  (이미 권한 검증 완료)              │
                     └────────────────────────────────────┘
```

## 감사 로깅

```rust
pub struct AuditLogger {
    log_file: Option<PathBuf>,
    enable_console: bool,
}

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub player_id: u64,
    pub action: String,
    pub resource: String,
    pub result: PolicyResult,
    pub ip_address: Option<String>,
    pub details: Option<String>,
}

impl AuditLogger {
    pub fn log(&self, entry: AuditEntry) {
        if self.enable_console {
            tracing::info!(
                player_id = entry.player_id,
                action = %entry.action,
                resource = %entry.resource,
                result = ?entry.result,
                "Audit: {} on {} by player {}",
                entry.action,
                entry.resource,
                entry.player_id,
            );
        }

        if let Some(path) = &self.log_file {
            let json = serde_json::to_string(&entry).unwrap();
            // 파일에 추가
        }
    }
}
```

**감사 로그 대상:**
- 로그인/로그아웃
- 캐릭터 생성/삭제
- 전투 시작
- 경매 등록/구매
- 관리자 권한 사용
- 권한 거부 이벤트
