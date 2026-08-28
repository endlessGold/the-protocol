# 02 - Runtime Core

## Overview

The Runtime Core is the central orchestrator of the Cross-Platform Game Runtime. It is NOT a server - it is a programmable engine that activates different capabilities based on configuration.

## Core Identity

```rust
pub struct Runtime {
    config: RuntimeConfig,
    capabilities: CapabilityManager,
    plugin_runtime: PluginRuntime,
    network: NetworkManager,
    session_manager: SessionManager,
    scheduler: Scheduler,
    event_bus: EventBus,
    security: SecurityManager,
}
```

## Runtime Modes

The Runtime operates in one or more modes simultaneously:

| Mode | Description | Primary Capabilities |
|------|-------------|---------------------|
| `server` | Hosts game worlds | TCP Listener, UDP Listener, HTTP, Session Manager, Plugin Runtime |
| `client` | Connects to runtime | TCP Client, UDP Client, HTTP Client, WebSocket Client, Plugin Runtime |
| `gateway` | Routes traffic | TCP Listener, UDP Listener, Auth, Routing, Proxy, Load Balancing |
| `peer` | Distributed node | Inter-node communication, State sync |
| `tool` | Admin/debug tool | CLI interface, Monitoring, Management |

## Configuration-Driven Mode Selection

```toml
# config.toml
[runtime]
mode = "server"  # server | client | gateway | peer | tool
name = "zone-1"

[capabilities]
tcp_listener = true
udp_listener = true
http_server = true
websocket = true
plugin_runtime = true
session_manager = true

[server]
bind_address = "0.0.0.0:7770"
max_connections = 1000

[plugins]
directory = "./plugins"
allowed = ["character", "combat", "inventory", "auction"]
```

## Runtime Lifecycle

```
1. Parse CLI arguments
2. Load configuration
3. Initialize Runtime Core
4. Load capabilities based on config
5. Initialize plugin runtime
6. Load and validate plugins
7. Start network listeners
8. Register plugin commands/events
9. Enter main event loop
10. On shutdown: unload plugins, close connections, cleanup
```

```rust
impl Runtime {
    pub async fn new(config: RuntimeConfig) -> Result<Self> {
        let capabilities = CapabilityManager::new(&config);
        let plugin_runtime = PluginRuntime::new(&config.plugins).await?;
        let network = NetworkManager::new(&config.network).await?;
        let session_manager = SessionManager::new(&config.server);
        let scheduler = Scheduler::new();
        let event_bus = EventBus::new();
        let security = SecurityManager::new(&config.security);

        Ok(Self {
            config,
            capabilities,
            plugin_runtime,
            network,
            session_manager,
            scheduler,
            event_bus,
            security,
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        self.plugin_runtime.load_all().await?;
        self.network.start_listeners().await?;
        self.enter_event_loop().await
    }

    async fn enter_event_loop(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(event) = self.event_bus.recv() => {
                    self.handle_event(event).await?;
                }
                Some(connection) = self.network.accept() => {
                    self.session_manager.create(connection).await?;
                }
                Some(task) = self.scheduler.next() => {
                    task.execute(&mut self.event_bus).await?;
                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
        self.shutdown().await
    }
}
```

## Module Dependency

```
Runtime Core
    │
    ├── Config (no dependencies)
    │
    ├── Security (depends on Config)
    │
    ├── Scheduler (no dependencies)
    │
    ├── EventBus (no dependencies)
    │
    ├── Plugin Runtime (depends on Config, Security, EventBus)
    │
    ├── Network (depends on Config, Protocol, Session)
    │
    ├── Session Manager (depends on Config, Security)
    │
    └── Routing (depends on EventBus, Plugin Runtime)
```

**Critical Rule**: Runtime Core does NOT depend on Domain or Application layers. Those are loaded through plugins.

## Startup Sequence Detail

```
┌─────────────────────────────────────┐
│ 1. CLI Argument Parsing (clap)      │
│    runtime server --config ./cfg    │
├─────────────────────────────────────┤
│ 2. Configuration Loading            │
│    TOML/JSON/YAML → RuntimeConfig   │
├─────────────────────────────────────┤
│ 3. Core Initialization              │
│    EventBus, Scheduler, Security    │
├─────────────────────────────────────┤
│ 4. Capability Resolution            │
│    Config → Activated Capabilities  │
├─────────────────────────────────────┤
│ 5. Plugin Runtime Init              │
│    Wasmtime engine, WASI setup      │
├─────────────────────────────────────┤
│ 6. Plugin Loading                   │
│    Discover → Validate → Load       │
├─────────────────────────────────────┤
│ 7. Network Startup                  │
│    Bind listeners, register routes  │
├─────────────────────────────────────┤
│ 8. Main Event Loop                  │
│    tokio::select! on all sources    │
└─────────────────────────────────────┘
```

## Error Handling

Runtime uses structured error types:

```rust
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Plugin error: {0}")]
    Plugin(#[from] PluginError),

    #[error("Network error: {0}")]
    Network(#[from] NetworkError),

    #[error("Session error: {0}")]
    Session(#[from] SessionError),

    #[error("Security error: {0}")]
    Security(#[from] SecurityError),

    #[error("Plugin not found: {0}")]
    PluginNotFound(String),

    #[error("Capability not available: {0}")]
    CapabilityUnavailable(String),
}
```

## Graceful Shutdown

```
Signal received (SIGINT/SIGTERM)
    │
    ▼
Stop accepting new connections
    │
    ▼
Notify all plugins of shutdown
    │
    ▼
Wait for in-flight operations (timeout: 30s)
    │
    ▼
Unload plugins (safe unload)
    │
    ▼
Close all sessions
    │
    ▼
Stop network listeners
    │
    ▼
Flush metrics and logs
    │
    ▼
Process exit
```

## Observability

Runtime exposes:

- **Metrics**: Connection count, message throughput, plugin execution time, memory usage
- **Tracing**: Distributed tracing through event flow
- **Logging**: Structured logging with levels (trace, debug, info, warn, error)
- **Health**: Health check endpoint for orchestrators

## References

- [01-architecture.md](01-architecture.md) - Overall architecture
- [03-capability.md](03-capability.md) - Capability system
- [04-plugin-system.md](04-plugin-system.md) - Plugin runtime
- [08-network.md](08-network.md) - Network layer