use crate::error::PluginError;
use crate::host;
use crate::manifest::{PluginManifest, PluginState};
use crate::state::{HostContext, HostState, SharedState};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use wasmtime::*;

struct CompiledModule {
    module: Module,
    manifest: PluginManifest,
}

struct LoadedPluginInstance {
    store: Store<HostState>,
    instance: Instance,
    manifest: PluginManifest,
    state: PluginState,
}

pub struct PluginEngine {
    engine: Engine,
    compiled: HashMap<String, CompiledModule>,
    instances: HashMap<String, LoadedPluginInstance>,
    shared_state: Arc<SharedState>,
    plugin_dir: PathBuf,
}

impl PluginEngine {
    pub fn new(plugin_dir: &str) -> Self {
        let mut config = Config::new();
        config.wasm_component_model(false);
        config.async_support(false);
        config.epoch_interruption(true);
        config.memory_init_cow(true);
        config.parallel_compilation(true);

        let engine = Engine::new(&config).expect("Failed to create wasmtime engine");

        Self {
            engine,
            compiled: HashMap::new(),
            instances: HashMap::new(),
            shared_state: Arc::new(SharedState::new()),
            plugin_dir: PathBuf::from(plugin_dir),
        }
    }

    pub fn with_shared_state(plugin_dir: &str, shared_state: Arc<SharedState>) -> Self {
        let mut config = Config::new();
        config.wasm_component_model(false);
        config.async_support(false);
        config.epoch_interruption(true);
        config.memory_init_cow(true);
        config.parallel_compilation(true);

        let engine = Engine::new(&config).expect("Failed to create wasmtime engine");

        Self {
            engine,
            compiled: HashMap::new(),
            instances: HashMap::new(),
            shared_state,
            plugin_dir: PathBuf::from(plugin_dir),
        }
    }

    pub fn shared_state(&self) -> Arc<SharedState> {
        self.shared_state.clone()
    }

    pub fn discover(&self) -> Result<Vec<PluginManifest>, PluginError> {
        let mut manifests = Vec::new();

        if !self.plugin_dir.exists() {
            tracing::warn!(
                "Plugin directory does not exist: {}",
                self.plugin_dir.display()
            );
            return Ok(manifests);
        }

        for entry in std::fs::read_dir(&self.plugin_dir).map_err(PluginError::Io)? {
            let entry = entry.map_err(PluginError::Io)?;
            let manifest_path = entry.path().join("plugin.toml");
            if manifest_path.exists() {
                let content = std::fs::read_to_string(&manifest_path).map_err(PluginError::Io)?;
                let manifest: PluginManifest =
                    toml::from_str(&content).map_err(|e| PluginError::InitFailed(e.to_string()))?;
                tracing::info!("Discovered plugin: {} v{}", manifest.name, manifest.version);
                manifests.push(manifest);
            }
        }

        Ok(manifests)
    }

    pub fn compile(&mut self, name: &str) -> Result<(), PluginError> {
        if self.compiled.contains_key(name) {
            return Ok(());
        }

        let manifests = self.discover()?;
        let manifest = manifests
            .into_iter()
            .find(|m| m.name == name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        // Reject an incompatible plugin before spending time compiling its
        // WASM module. See `manifest::validate_api_version` for why this
        // wasn't happening before.
        crate::manifest::validate_api_version(
            &manifest.api_version,
            crate::manifest::RUNTIME_API_VERSION,
        )?;

        let wasm_path = self.plugin_dir.join(name).join("plugin.wasm");
        if !wasm_path.exists() {
            return Err(PluginError::NotFound(format!(
                "WASM file not found: {}",
                wasm_path.display()
            )));
        }

        let wasm_bytes = std::fs::read(&wasm_path).map_err(PluginError::Io)?;

        let module = Module::new(&self.engine, &wasm_bytes)
            .map_err(|e| PluginError::Compilation(e.to_string()))?;

        self.compiled
            .insert(name.to_string(), CompiledModule { module, manifest });

        tracing::info!("Compiled plugin: {}", name);
        Ok(())
    }

    fn build_linker(engine: &Engine) -> Linker<HostState> {
        let mut linker = Linker::<HostState>::new(engine);

        linker
            .func_wrap(
                "plugin_host",
                "log",
                |mut caller: Caller<'_, HostState>, level: i32, ptr: u32, len: u32| {
                    host::host_log(&mut caller, level, ptr, len);
                },
            )
            .expect("Failed to link log");

        linker
            .func_wrap(
                "plugin_host",
                "storage_get",
                |mut caller: Caller<'_, HostState>, key_ptr: u32, key_len: u32| -> i64 {
                    host::host_storage_get(&mut caller, key_ptr, key_len).unwrap_or(-1)
                },
            )
            .expect("Failed to link storage_get");

        linker
            .func_wrap(
                "plugin_host",
                "storage_set",
                |mut caller: Caller<'_, HostState>,
                 key_ptr: u32,
                 key_len: u32,
                 val_ptr: u32,
                 val_len: u32|
                 -> i32 {
                    host::host_storage_set(&mut caller, key_ptr, key_len, val_ptr, val_len)
                        .unwrap_or(-1)
                },
            )
            .expect("Failed to link storage_set");

        linker
            .func_wrap(
                "plugin_host",
                "storage_delete",
                |mut caller: Caller<'_, HostState>, key_ptr: u32, key_len: u32| -> i32 {
                    host::host_storage_delete(&mut caller, key_ptr, key_len).unwrap_or(-1)
                },
            )
            .expect("Failed to link storage_delete");

        linker
            .func_wrap(
                "plugin_host",
                "emit_event",
                |mut caller: Caller<'_, HostState>,
                 type_ptr: u32,
                 type_len: u32,
                 data_ptr: u32,
                 data_len: u32|
                 -> i32 {
                    host::host_emit_event(&mut caller, type_ptr, type_len, data_ptr, data_len)
                        .unwrap_or(-1)
                },
            )
            .expect("Failed to link emit_event");

        linker
            .func_wrap(
                "plugin_host",
                "player_get",
                |mut caller: Caller<'_, HostState>, player_id: i64| -> i64 {
                    host::host_player_get(&mut caller, player_id).unwrap_or(-1)
                },
            )
            .expect("Failed to link player_get");

        linker
            .func_wrap(
                "plugin_host",
                "player_update",
                |mut caller: Caller<'_, HostState>,
                 player_id: i64,
                 data_ptr: u32,
                 data_len: u32|
                 -> i32 {
                    host::host_player_update(&mut caller, player_id, data_ptr, data_len)
                        .unwrap_or(-1)
                },
            )
            .expect("Failed to link player_update");

        linker
            .func_wrap(
                "plugin_host",
                "inventory_get",
                |mut caller: Caller<'_, HostState>, player_id: i64| -> i64 {
                    host::host_inventory_get(&mut caller, player_id).unwrap_or(-1)
                },
            )
            .expect("Failed to link inventory_get");

        linker
            .func_wrap(
                "plugin_host",
                "inventory_add_item",
                |mut caller: Caller<'_, HostState>,
                 player_id: i64,
                 item_id: i64,
                 count: i32|
                 -> i32 {
                    host::host_inventory_add_item(&mut caller, player_id, item_id, count)
                        .unwrap_or(-1)
                },
            )
            .expect("Failed to link inventory_add_item");

        linker
            .func_wrap(
                "plugin_host",
                "inventory_remove_item",
                |mut caller: Caller<'_, HostState>,
                 player_id: i64,
                 item_id: i64,
                 count: i32|
                 -> i32 {
                    host::host_inventory_remove_item(&mut caller, player_id, item_id, count)
                        .unwrap_or(-1)
                },
            )
            .expect("Failed to link inventory_remove_item");

        linker
            .func_wrap(
                "plugin_host",
                "combat_start",
                |mut caller: Caller<'_, HostState>, attacker_id: i64, defender_id: i64| -> i64 {
                    host::host_combat_start(&mut caller, attacker_id, defender_id).unwrap_or(-1)
                },
            )
            .expect("Failed to link combat_start");

        linker
            .func_wrap(
                "plugin_host",
                "combat_action",
                |mut caller: Caller<'_, HostState>,
                 combat_id: i64,
                 action_ptr: u32,
                 action_len: u32|
                 -> i32 {
                    host::host_combat_action(&mut caller, combat_id, action_ptr, action_len)
                        .unwrap_or(-1)
                },
            )
            .expect("Failed to link combat_action");

        linker
            .func_wrap(
                "plugin_host",
                "send_to_client",
                |mut caller: Caller<'_, HostState>,
                 player_id: i64,
                 msg_ptr: u32,
                 msg_len: u32|
                 -> i32 {
                    host::host_send_to_client(&mut caller, player_id, msg_ptr, msg_len)
                        .unwrap_or(-1)
                },
            )
            .expect("Failed to link send_to_client");

        linker
            .func_wrap(
                "plugin_host",
                "broadcast_to_room",
                |mut caller: Caller<'_, HostState>,
                 room_id: i64,
                 msg_ptr: u32,
                 msg_len: u32|
                 -> i32 {
                    host::host_broadcast_to_room(&mut caller, room_id, msg_ptr, msg_len)
                        .unwrap_or(-1)
                },
            )
            .expect("Failed to link broadcast_to_room");

        linker
    }

    pub fn instantiate(&mut self, name: &str, context: HostContext) -> Result<(), PluginError> {
        if self.instances.contains_key(name) {
            return Err(PluginError::Lifecycle(format!(
                "Plugin {} already instantiated",
                name
            )));
        }

        let compiled = self
            .compiled
            .get(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        let manifest = compiled.manifest.clone();
        let fuel_limit = manifest.resources.fuel_limit;

        let host_state = HostState::new(context, self.shared_state.clone());

        let mut store = Store::new(&self.engine, host_state);
        store
            .set_fuel(fuel_limit)
            .map_err(|e| PluginError::Instantiation(e.to_string()))?;

        let linker = Self::build_linker(&self.engine);

        let instance = linker
            .instantiate(&mut store, &compiled.module)
            .map_err(|e| PluginError::Instantiation(e.to_string()))?;

        let plugin_state = PluginState::Loaded;

        self.instances.insert(
            name.to_string(),
            LoadedPluginInstance {
                store,
                instance,
                manifest,
                state: plugin_state,
            },
        );

        tracing::info!("Instantiated plugin: {}", name);
        Ok(())
    }

    pub fn initialize(&mut self, name: &str) -> Result<(), PluginError> {
        let instance = self
            .instances
            .get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        if instance.state != PluginState::Loaded {
            return Err(PluginError::Lifecycle(format!(
                "Plugin {} is not in Loaded state",
                name
            )));
        }

        let init_fn = instance
            .instance
            .get_typed_func::<(), i32>(&mut instance.store, "plugin_init")
            .map_err(|e| PluginError::FunctionNotFound(e.to_string()))?;

        let result = init_fn
            .call(&mut instance.store, ())
            .map_err(|e| PluginError::RuntimeError(e.to_string()))?;

        if result != 0 {
            return Err(PluginError::InitFailed(format!(
                "plugin_init returned {}",
                result
            )));
        }

        instance.state = PluginState::Initialized;
        tracing::info!("Initialized plugin: {}", name);
        Ok(())
    }

    pub fn enable(&mut self, name: &str) -> Result<(), PluginError> {
        let instance = self
            .instances
            .get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        if instance.state != PluginState::Initialized {
            return Err(PluginError::Lifecycle(format!(
                "Plugin {} is not in Initialized state",
                name
            )));
        }

        let enable_fn = instance
            .instance
            .get_typed_func::<(), i32>(&mut instance.store, "plugin_enable")
            .map_err(|e| PluginError::FunctionNotFound(e.to_string()))?;

        let result = enable_fn
            .call(&mut instance.store, ())
            .map_err(|e| PluginError::RuntimeError(e.to_string()))?;

        if result != 0 {
            return Err(PluginError::InitFailed(format!(
                "plugin_enable returned {}",
                result
            )));
        }

        instance.state = PluginState::Enabled;
        tracing::info!("Enabled plugin: {}", name);
        Ok(())
    }

    pub fn disable(&mut self, name: &str) -> Result<(), PluginError> {
        let instance = self
            .instances
            .get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        if instance.state != PluginState::Enabled {
            return Err(PluginError::Lifecycle(format!(
                "Plugin {} is not in Enabled state",
                name
            )));
        }

        let disable_fn = instance
            .instance
            .get_typed_func::<(), i32>(&mut instance.store, "plugin_disable")
            .map_err(|e| PluginError::FunctionNotFound(e.to_string()))?;

        let result = disable_fn
            .call(&mut instance.store, ())
            .map_err(|e| PluginError::RuntimeError(e.to_string()))?;

        if result != 0 {
            return Err(PluginError::InitFailed(format!(
                "plugin_disable returned {}",
                result
            )));
        }

        instance.state = PluginState::Disabled;
        tracing::info!("Disabled plugin: {}", name);
        Ok(())
    }

    pub fn unload(&mut self, name: &str) -> Result<(), PluginError> {
        if let Some(mut instance) = self.instances.remove(name) {
            if instance.state == PluginState::Enabled {
                let _ = {
                    let disable_fn = instance
                        .instance
                        .get_typed_func::<(), i32>(&mut instance.store, "plugin_disable");
                    if let Ok(f) = disable_fn {
                        let _ = f.call(&mut instance.store, ());
                    }
                };
            }

            let _ = {
                let unload_fn = instance
                    .instance
                    .get_typed_func::<(), i32>(&mut instance.store, "plugin_unload");
                if let Ok(f) = unload_fn {
                    let _ = f.call(&mut instance.store, ());
                }
            };

            tracing::info!("Unloaded plugin: {}", name);
        }
        Ok(())
    }

    pub fn handle_command(
        &mut self,
        name: &str,
        command: &str,
        args: &str,
        player_id: i64,
    ) -> Result<i32, PluginError> {
        let instance = self
            .instances
            .get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        if instance.state != PluginState::Enabled {
            return Err(PluginError::Lifecycle(format!(
                "Plugin {} is not enabled",
                name
            )));
        }

        let cmd_bytes = command.as_bytes();
        let args_bytes = args.as_bytes();

        let cmd_ptr = allocate_in_store(
            &mut instance.store,
            &instance.instance,
            cmd_bytes.len() as u32,
        )?;
        write_to_store(&mut instance.store, &instance.instance, cmd_ptr, cmd_bytes)?;

        let args_ptr = allocate_in_store(
            &mut instance.store,
            &instance.instance,
            args_bytes.len() as u32,
        )?;
        write_to_store(
            &mut instance.store,
            &instance.instance,
            args_ptr,
            args_bytes,
        )?;

        let handle_fn = instance
            .instance
            .get_typed_func::<(i32, i32, i32, i32, i64), i32>(&mut instance.store, "handle_command")
            .map_err(|e| PluginError::FunctionNotFound(e.to_string()))?;

        let result = handle_fn
            .call(
                &mut instance.store,
                (
                    cmd_ptr as i32,
                    cmd_bytes.len() as i32,
                    args_ptr as i32,
                    args_bytes.len() as i32,
                    player_id,
                ),
            )
            .map_err(|e| PluginError::RuntimeError(e.to_string()))?;

        free_in_store(
            &mut instance.store,
            &instance.instance,
            cmd_ptr,
            cmd_bytes.len() as u32,
        )?;
        free_in_store(
            &mut instance.store,
            &instance.instance,
            args_ptr,
            args_bytes.len() as u32,
        )?;

        Ok(result)
    }

    pub fn handle_event(
        &mut self,
        name: &str,
        event_type: &str,
        data: &[u8],
    ) -> Result<i32, PluginError> {
        let instance = self
            .instances
            .get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        if instance.state != PluginState::Enabled {
            return Err(PluginError::Lifecycle(format!(
                "Plugin {} is not enabled",
                name
            )));
        }

        let type_bytes = event_type.as_bytes();

        let type_ptr = allocate_in_store(
            &mut instance.store,
            &instance.instance,
            type_bytes.len() as u32,
        )?;
        write_to_store(
            &mut instance.store,
            &instance.instance,
            type_ptr,
            type_bytes,
        )?;

        let data_ptr =
            allocate_in_store(&mut instance.store, &instance.instance, data.len() as u32)?;
        write_to_store(&mut instance.store, &instance.instance, data_ptr, data)?;

        let handle_fn = instance
            .instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut instance.store, "handle_event")
            .map_err(|e| PluginError::FunctionNotFound(e.to_string()))?;

        let result = handle_fn
            .call(
                &mut instance.store,
                (
                    type_ptr as i32,
                    type_bytes.len() as i32,
                    data_ptr as i32,
                    data.len() as i32,
                ),
            )
            .map_err(|e| PluginError::RuntimeError(e.to_string()))?;

        free_in_store(
            &mut instance.store,
            &instance.instance,
            type_ptr,
            type_bytes.len() as u32,
        )?;
        free_in_store(
            &mut instance.store,
            &instance.instance,
            data_ptr,
            data.len() as u32,
        )?;

        Ok(result)
    }

    pub fn get_messages(&self, player_id: i64) -> Vec<String> {
        self.shared_state
            .messages
            .get(&player_id)
            .map(|r| r.value().clone())
            .unwrap_or_default()
    }

    pub fn clear_messages(&self, player_id: i64) {
        self.shared_state.messages.remove(&player_id);
    }

    pub fn enabled_plugins(&self) -> Vec<String> {
        self.instances
            .iter()
            .filter(|(_, p)| p.state == PluginState::Enabled)
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn plugin_state(&self, name: &str) -> Option<PluginState> {
        self.instances.get(name).map(|p| p.state.clone())
    }
}

fn allocate_in_store(
    store: &mut Store<HostState>,
    instance: &Instance,
    size: u32,
) -> Result<u32, PluginError> {
    let alloc_fn = instance
        .get_export(&mut *store, "allocate_buffer")
        .and_then(|e| e.into_func())
        .ok_or_else(|| PluginError::FunctionNotFound("allocate_buffer".into()))?;

    let mut results = [Val::I32(0)];
    alloc_fn
        .call(&mut *store, &[Val::I32(size as i32)], &mut results)
        .map_err(|e| PluginError::RuntimeError(e.to_string()))?;

    match results[0] {
        Val::I32(ptr) => Ok(ptr as u32),
        _ => Err(PluginError::RuntimeError("invalid return type".into())),
    }
}

fn write_to_store(
    store: &mut Store<HostState>,
    instance: &Instance,
    ptr: u32,
    data: &[u8],
) -> Result<(), PluginError> {
    let memory = instance
        .get_export(&mut *store, "memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| PluginError::Memory("memory export not found".into()))?;

    memory
        .write(&mut *store, ptr as usize, data)
        .map_err(|e| PluginError::Memory(e.to_string()))?;
    Ok(())
}

fn free_in_store(
    store: &mut Store<HostState>,
    instance: &Instance,
    ptr: u32,
    size: u32,
) -> Result<(), PluginError> {
    let free_fn = instance
        .get_export(&mut *store, "free_buffer")
        .and_then(|e| e.into_func());

    if let Some(free_fn) = free_fn {
        let _ = free_fn.call(
            &mut *store,
            &[Val::I32(ptr as i32), Val::I32(size as i32)],
            &mut [],
        );
    }

    Ok(())
}
