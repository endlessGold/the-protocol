# 04 - Plugin System

## Overview

The Plugin System manages the lifecycle of WASM plugins within the Runtime. Plugins are the primary way game logic is implemented, keeping the Core Runtime generic and game-agnostic.

## Plugin Lifecycle

```
Discover → Validate → Resolve Dependencies → Load → Initialize → Enable → [Running] → Disable → Unload
```

### Phase Details

| Phase | Description | Failure Behavior |
|-------|-------------|-----------------|
| Discover | Scan plugin directory, read manifests | Skip invalid manifests |
| Validate | Check API version, permissions, signatures | Reject incompatible plugins |
| Resolve | Check dependency graph, detect cycles | Fail if critical dependency missing |
| Load | Instantiate WASM module | Fail plugin, continue others |
| Initialize | Call plugin init function | Fail plugin, continue others |
| Enable | Plugin is active, can handle events | Disable on error |
| Disable | Graceful shutdown of plugin | Force unload on timeout |
| Unload | Release WASM resources | Force cleanup |

## Plugin Manifest

```toml
# plugin.toml
[plugin]
name = "combat"
version = "1.0.0"
description = "Combat system plugin"
author = "The Protocol Team"
api_version = "1.0"
runtime_version = ">=1.0.0"

[plugin.permissions]
required = ["player.read", "inventory.read", "combat.modify"]
optional = ["database.write", "cache.write"]

[plugin.resources]
memory_limit = "64MB"
execution_limit = "100ms"
storage_quota = "10MB"

[plugin.dependencies]
"character" = ">=1.0.0"

[plugin.metadata]
category = "gameplay"
tags = ["combat", "pvp", "pve"]
```

## Plugin Structure

```
plugins/
    combat/
        plugin.toml          # Manifest
        combat.wasm          # Compiled WASM binary
        lib.rs               # Source (for reference)
    inventory/
        plugin.toml
        inventory.wasm
    character/
        plugin.toml
        character.wasm
```

## Plugin Manager

```rust
pub struct PluginManager {
    plugins: HashMap<String, LoadedPlugin>,
    manifest_dir: PathBuf,
    capability_manager: Arc<RwLock<CapabilityManager>>,
    event_bus: EventBus,
}

pub struct LoadedPlugin {
    name: String,
    manifest: PluginManifest,
    instance: WasmInstance,
    state: PluginState,
    loaded_at: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginState {
    Discovered,
    Validated,
    Loaded,
    Initialized,
    Enabled,
    Disabled,
    Error(String),
}

impl PluginManager {
    pub async fn discover(&mut self) -> Result<Vec<PluginManifest>> {
        let mut manifests = Vec::new();
        for entry in fs::read_dir(&self.manifest_dir).await? {
            let path = entry?.path();
            if path.join("plugin.toml").exists() {
                let manifest = PluginManifest::load(&path.join("plugin.toml"))?;
                manifests.push(manifest);
            }
        }
        Ok(manifests)
    }

    pub async fn validate(&self, manifest: &PluginManifest) -> Result<()> {
        // Check API version compatibility
        if !manifest.api_version_compatible(&RUNTIME_API_VERSION) {
            return Err(PluginError::IncompatibleApiVersion {
                plugin: manifest.name.clone(),
                required: manifest.api_version.clone(),
                available: RUNTIME_API_VERSION.clone(),
            });
        }

        // Check runtime version requirement
        if !manifest.runtime_version_satisfied(&RUNTIME_VERSION) {
            return Err(PluginError::IncompatibleRuntimeVersion {
                plugin: manifest.name.clone(),
                required: manifest.runtime_version.clone(),
                available: RUNTIME_VERSION.clone(),
            });
        }

        // Check permissions are grantable
        let cap_manager = self.capability_manager.read().await;
        for perm in &manifest.permissions.required {
            if !cap_manager.can_grant(&manifest.name, perm) {
                return Err(PluginError::PermissionDenied {
                    plugin: manifest.name.clone(),
                    permission: perm.clone(),
                });
            }
        }

        Ok(())
    }

    pub async fn load(&mut self, manifest: &PluginManifest) -> Result<()> {
        let wasm_path = self.manifest_dir.join(&manifest.name).join(format!("{}.wasm", manifest.name));
        let wasm_bytes = fs::read(&wasm_path).await?;

        let instance = self.plugin_runtime.instantiate(
            &wasm_bytes,
            manifest.resources.memory_limit,
        ).await?;

        self.plugins.insert(manifest.name.clone(), LoadedPlugin {
            name: manifest.name.clone(),
            manifest: manifest.clone(),
            instance,
            state: PluginState::Loaded,
            loaded_at: Instant::now(),
        });

        Ok(())
    }

    pub async fn initialize(&mut self, name: &str) -> Result<()> {
        let plugin = self.plugins.get_mut(name).ok_or(PluginError::NotFound(name.into()))?;
        plugin.instance.call_init().await?;
        plugin.state = PluginState::Initialized;
        Ok(())
    }

    pub async fn enable(&mut self, name: &str) -> Result<()> {
        let plugin = self.plugins.get_mut(name).ok_or(PluginError::NotFound(name.into()))?;
        plugin.instance.call_enable().await?;
        plugin.state = PluginState::Enabled;
        Ok(())
    }

    pub async fn disable(&mut self, name: &str) -> Result<()> {
        let plugin = self.plugins.get_mut(name).ok_or(PluginError::NotFound(name.into()))?;
        plugin.instance.call_disable().await?;
        plugin.state = PluginState::Disabled;
        Ok(())
    }

    pub async fn unload(&mut self, name: &str) -> Result<()> {
        if let Some(mut plugin) = self.plugins.remove(name) {
            if plugin.state == PluginState::Enabled {
                plugin.instance.call_disable().await?;
            }
            plugin.instance.call_unload().await?;
            drop(plugin.instance);
        }
        Ok(())
    }

    pub fn get_enabled_plugins(&self) -> Vec<&LoadedPlugin> {
        self.plugins.values()
            .filter(|p| p.state == PluginState::Enabled)
            .collect()
    }
}
```

## Dependency Resolution

```rust
pub fn resolve_dependencies(
    manifests: &[PluginManifest],
) -> Result<Vec<PluginManifest>> {
    let mut graph = DependencyGraph::new();

    for manifest in manifests {
        graph.add_node(&manifest.name);
        for dep in &manifest.dependencies {
            graph.add_edge(&manifest.name, &dep.name);
        }
    }

    // Detect cycles
    if let Some(cycle) = graph.find_cycle() {
        return Err(PluginError::CircularDependency(cycle));
    }

    // Topological sort
    let order = graph.topological_sort();
    Ok(order.into_iter()
        .map(|name| manifests.iter().find(|m| m.name == name).unwrap().clone())
        .collect())
}
```

## Hot Reload (Development)

```rust
#[cfg(debug_assertions)]
impl PluginManager {
    pub async fn watch_and_reload(&mut self) -> Result<()> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        let mut watcher = notify::recommended_watcher(move |res| {
            if let Ok(event) = res {
                let _ = tx.blocking_send(event);
            }
        })?;

        watcher.watch(
            self.manifest_dir.as_ref(),
            RecursiveMode::Recursive,
        )?;

        while let Some(event) = rx.recv().await {
            if let Some(path) = event.paths.first() {
                if path.extension() == Some(OsStr::new("wasm")) {
                    let plugin_name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default();

                    tracing::info!("Hot reloading plugin: {}", plugin_name);

                    // Safe unload → reload cycle
                    if self.plugins.contains_key(plugin_name) {
                        self.disable(plugin_name).await?;
                        self.unload(plugin_name).await?;
                    }

                    if let Some(manifest) = self.discover_one(plugin_name).await? {
                        self.validate(&manifest).await?;
                        self.load(&manifest).await?;
                        self.initialize(&manifest.name).await?;
                        self.enable(&manifest.name).await?;
                    }
                }
            }
        }

        Ok(())
    }
}
```

## Plugin Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Incompatible API version: plugin {plugin} requires {required}, runtime has {available}")]
    IncompatibleApiVersion {
        plugin: String,
        required: String,
        available: String,
    },

    #[error("Incompatible runtime version: plugin {plugin} requires {required}, runtime is {available}")]
    IncompatibleRuntimeVersion {
        plugin: String,
        required: String,
        available: String,
    },

    #[error("Permission denied: plugin {plugin} cannot use {permission}")]
    PermissionDenied {
        plugin: String,
        permission: String,
    },

    #[error("Circular dependency: {0}")]
    CircularDependency(Vec<String>),

    #[error("WASM error: {0}")]
    Wasm(String),

    #[error("Plugin initialization failed: {0}")]
    InitFailed(String),
}
```

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Plugin format | WASM | Platform-independent, sandboxed |
| Lifecycle | Explicit phases | Predictable, debuggable |
| Dependencies | Topological ordering | Prevent initialization order issues |
| Hot reload | Dev-only | Safety in production, speed in dev |
| Error handling | Plugin-level isolation | One bad plugin doesn't crash Runtime |
| Memory limits | Per-plugin | Prevent resource exhaustion |

## References

- [03-capability.md](03-capability.md) - Permission model
- [05-plugin-api.md](05-plugin-api.md) - Plugin API contract
- [06-wasm.md](06-wasm.md) - WASM runtime details
