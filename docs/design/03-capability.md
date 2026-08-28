# 03 - Capability System

## Overview

The Capability System controls what the Runtime can do and what plugins are allowed to access. It is a permission model that bridges runtime configuration and plugin sandboxing.

## Two Levels of Capabilities

### 1. Runtime Capabilities
Determine which features the Runtime itself activates.

### 2. Plugin Capabilities
Determine what a specific plugin is allowed to do within the Runtime.

## Runtime Capabilities

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    // Transport
    pub tcp_listener: bool,
    pub udp_listener: bool,
    pub tcp_client: bool,
    pub udp_client: bool,
    pub http_server: bool,
    pub http_client: bool,
    pub websocket_server: bool,
    pub websocket_client: bool,

    // Core
    pub plugin_runtime: bool,
    pub session_manager: bool,
    pub scheduler: bool,
    pub event_bus: bool,

    // Infrastructure
    pub database: bool,
    pub cache: bool,
    pub metrics: bool,
}
```

### Capability Profiles

```toml
# Server profile
[runtime.capabilities]
tcp_listener = true
udp_listener = true
http_server = true
websocket_server = true
plugin_runtime = true
session_manager = true
scheduler = true
event_bus = true
database = true
cache = true
metrics = true

# Client profile
[runtime.capabilities]
tcp_client = true
udp_client = true
http_client = true
websocket_client = true
plugin_runtime = true
session_manager = true
event_bus = true

# Gateway profile
[runtime.capabilities]
tcp_listener = true
udp_listener = true
http_server = true
plugin_runtime = true
session_manager = true
scheduler = true
event_bus = true
metrics = true

# Minimal tool profile
[runtime.capabilities]
http_client = true
metrics = true
```

## Plugin Capabilities

Each plugin declares what it needs. The Runtime grants or denies based on configuration.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCapabilities {
    pub permissions: Vec<Permission>,
    pub memory_limit: Option<usize>,      // bytes
    pub execution_limit: Option<Duration>, // max execution time
    pub storage_access: bool,
    pub logging_access: bool,
    pub network_access: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    // Domain permissions
    PlayerRead,
    PlayerModify,
    InventoryRead,
    InventoryModify,
    CombatRead,
    CombatModify,
    WorldRead,
    WorldModify,
    AuctionRead,
    AuctionModify,
    GuildRead,
    GuildModify,

    // System permissions
    DatabaseRead,
    DatabaseWrite,
    CacheRead,
    CacheWrite,
    ScheduleTimer,
    EmitEvent,
    RegisterCommand,
    RegisterRoute,

    // Custom
    Custom(String),
}
```

### Plugin Permission Declaration

```toml
# plugins/combat/plugin.toml
[plugin]
name = "combat"
version = "1.0.0"
api_version = "1.0"

[plugin.permissions]
required = [
    "player.read",
    "inventory.read",
    "combat.modify",
]
optional = [
    "database.write",
    "cache.write",
]

[plugin.resources]
memory_limit = "64MB"
execution_limit = "100ms"
```

## Permission Resolution

```
Plugin requests permission
        │
        ▼
Runtime checks: Is capability available?
        │
        ├── No → Deny (plugin cannot use this feature)
        │
        └── Yes → Check plugin's declared permissions
                  │
                  ├── Not declared → Deny
                  │
                  ├── Declared as required → Grant if available
                  │
                  └── Declared as optional → Grant or deny based on policy
```

## Capability Manager

```rust
pub struct CapabilityManager {
    runtime_caps: RuntimeCapabilities,
    plugin_caps: HashMap<String, PluginCapabilities>,
    policy: CapabilityPolicy,
}

impl CapabilityManager {
    pub fn new(config: &RuntimeConfig) -> Self {
        Self {
            runtime_caps: RuntimeCapabilities::from_config(&config.capabilities),
            plugin_caps: HashMap::new(),
            policy: CapabilityPolicy::from_config(&config.security),
        }
    }

    pub fn has_runtime_capability(&self, cap: &str) -> bool {
        self.runtime_caps.has(cap)
    }

    pub fn register_plugin(&mut self, name: &str, caps: PluginCapabilities) {
        self.plugin_caps.insert(name.to_string(), caps);
    }

    pub fn check_permission(&self, plugin: &str, permission: &Permission) -> bool {
        // 1. Plugin must have declared this permission
        // 2. Runtime must have the capability to grant it
        // 3. Policy must allow it
        self.plugin_caps.get(plugin)
            .map(|caps| caps.permissions.contains(permission))
            .unwrap_or(false)
            && self.runtime_caps.supports(permission)
            && self.policy.allows(plugin, permission)
    }
}
```

## Security Policy

```toml
[security]
# Default policy for plugin permissions
default_allow = false

# Specific overrides
[security.policies]
"combat" = { player = "read", inventory = "read", combat = "modify" }
"character" = { player = "modify", inventory = "read" }
"auction" = { player = "read", inventory = "read", database = "write" }

# Restricted plugins need explicit approval
[security.approved_plugins]
"combat" = true
"character" = true
"inventory" = true
"auction" = false  # Not yet approved
```

## Capability + WASM Sandbox Integration

```
┌─────────────────────────────────────────┐
│              WASM Sandbox               │
│                                         │
│  Plugin Code (untrusted)                │
│  ┌─────────────────────────────────┐    │
│  │ game logic                      │    │
│  └──────────┬──────────────────────┘    │
│             │                           │
│  ┌──────────▼──────────────────────┐    │
│  │ Host Function Interface         │    │
│  │ (capability-checked)            │    │
│  │  - player.read()  → ALLOW      │    │
│  │  - db.write()     → DENY       │    │
│  │  - inventory.mod()→ ALLOW      │    │
│  └─────────────────────────────────┘    │
│                                         │
│  Memory: capped at 64MB                 │
│  Execution: capped at 100ms per call    │
│  Storage: isolated per plugin           │
└─────────────────────────────────────────┘
```

## Capability Checking Flow

```
Plugin calls host function: player.read(player_id)
    │
    ▼
WASM host function handler invoked
    │
    ▼
CapabilityManager.check_permission("combat", &Permission::PlayerRead)
    │
    ├── Permission not declared by plugin → Return error
    ├── Runtime capability not available → Return error
    ├── Policy denies → Return error
    └── All checks pass → Execute function, return result
```

## Dynamic Capabilities (Future)

In distributed mode, capabilities can change at runtime:

```
Runtime Node starts with: server capability
    │
    ▼
Node receives: "activate gateway capability"
    │
    ▼
Runtime dynamically enables gateway features
    │
    ▼
New connections can now use gateway routing
```

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Permission model | Declared + Verified | Plugins declare needs, Runtime verifies |
| Default policy | Deny all | Security-first, explicit grant required |
| Granularity | Per-permission | Fine-grained control over plugin access |
| Memory limits | Per-plugin WASM | Prevent memory leaks and DoS |
| Execution limits | Per-call timeout | Prevent infinite loops in plugins |

## References

- [04-plugin-system.md](04-plugin-system.md) - Plugin lifecycle
- [05-plugin-api.md](05-plugin-api.md) - Plugin API contract
- [06-wasm.md](06-wasm.md) - WASM sandbox details
- [16-security.md](16-security.md) - Full security model
