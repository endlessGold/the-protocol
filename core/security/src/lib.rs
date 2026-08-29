use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Plugin not authorized: {0}")]
    UnauthorizedPlugin(String),

    #[error("Rate limited")]
    RateLimited,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
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
    DatabaseRead,
    DatabaseWrite,
    CacheRead,
    CacheWrite,
    ScheduleTimer,
    EmitEvent,
    RegisterCommand,
    Custom(String),
}

impl Permission {
    // Not `std::str::FromStr`: that returns Result, and an unrecognized
    // name here is ordinary user input rather than an error worth a type.
    #[allow(clippy::should_implement_trait)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCapabilities {
    pub permissions: Vec<Permission>,
    pub memory_limit: Option<usize>,
    pub execution_limit_ms: Option<u64>,
    pub storage_access: bool,
}

impl Default for PluginCapabilities {
    fn default() -> Self {
        Self {
            permissions: Vec::new(),
            memory_limit: Some(64 * 1024 * 1024), // 64MB
            execution_limit_ms: Some(100),
            storage_access: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CapabilityManager {
    runtime_capabilities: RuntimeCapabilities,
    plugin_capabilities: DashMap<String, PluginCapabilities>,
}

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

impl Default for RuntimeCapabilities {
    fn default() -> Self {
        Self {
            tcp_listener: true,
            udp_listener: false,
            tcp_client: false,
            udp_client: false,
            http_server: false,
            http_client: false,
            websocket_server: false,
            websocket_client: false,
            plugin_runtime: true,
            session_manager: true,
            scheduler: true,
            event_bus: true,
            database: false,
            cache: false,
            metrics: false,
        }
    }
}

impl RuntimeCapabilities {
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

impl CapabilityManager {
    pub fn new(runtime_caps: RuntimeCapabilities) -> Self {
        Self {
            runtime_capabilities: runtime_caps,
            plugin_capabilities: DashMap::new(),
        }
    }

    pub fn register_plugin(&self, name: &str, caps: PluginCapabilities) {
        self.plugin_capabilities.insert(name.to_string(), caps);
    }

    pub fn check_permission(&self, plugin: &str, permission: &Permission) -> bool {
        if let Some(caps) = self.plugin_capabilities.get(plugin) {
            caps.permissions.contains(permission)
        } else {
            false
        }
    }

    /// Whether this runtime provides `cap`, one of the field names on
    /// `RuntimeCapabilities` (e.g. "tcp_listener", "plugin_runtime").
    ///
    /// Returns false for an unknown name rather than defaulting to true:
    /// a typo'd capability should read as absent, not as universally
    /// granted. This previously ignored its argument and returned true
    /// unconditionally, which made every capability check meaningless.
    pub fn has_runtime_capability(&self, cap: &str) -> bool {
        let caps = &self.runtime_capabilities;
        match cap {
            "tcp_listener" => caps.tcp_listener,
            "udp_listener" => caps.udp_listener,
            "tcp_client" => caps.tcp_client,
            "udp_client" => caps.udp_client,
            "http_server" => caps.http_server,
            "http_client" => caps.http_client,
            "websocket_server" => caps.websocket_server,
            "websocket_client" => caps.websocket_client,
            "plugin_runtime" => caps.plugin_runtime,
            "session_manager" => caps.session_manager,
            "scheduler" => caps.scheduler,
            "event_bus" => caps.event_bus,
            "database" => caps.database,
            "cache" => caps.cache,
            "metrics" => caps.metrics,
            unknown => {
                tracing::warn!("Unknown runtime capability queried: {}", unknown);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_capability_reflects_the_profile() {
        let server = CapabilityManager::new(RuntimeCapabilities::server());
        assert!(server.has_runtime_capability("tcp_listener"));
        assert!(server.has_runtime_capability("plugin_runtime"));
    }

    #[test]
    fn unknown_capability_is_denied_not_granted() {
        // The old stub returned true for everything, so a typo silently
        // granted a capability. Absent must mean absent.
        let mgr = CapabilityManager::new(RuntimeCapabilities::server());
        assert!(!mgr.has_runtime_capability("tcp_listner"));
        assert!(!mgr.has_runtime_capability(""));
    }

    #[test]
    fn profiles_actually_differ() {
        let server = CapabilityManager::new(RuntimeCapabilities::server());
        let client = CapabilityManager::new(RuntimeCapabilities::client());
        // A client isn't a listener; if these ever come back equal the
        // profiles have collapsed into each other.
        assert_ne!(
            server.has_runtime_capability("tcp_listener"),
            client.has_runtime_capability("tcp_listener")
        );
    }
}
