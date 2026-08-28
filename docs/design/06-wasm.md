# 06 - WASM Integration

## Overview

WASM (WebAssembly) is the plugin execution environment. It provides platform independence, memory safety, and capability-based sandboxing.

## WASM Runtime Selection

### Comparison

| Feature | Wasmtime | Wasmer | Wasmer (Singlepass) |
|---------|----------|--------|---------------------|
| Performance | Excellent (Cranelift) | Good (Cranelift/LLVM/Singlepass) | Best for JIT |
| Embedding API | Excellent (Rust-native) | Good | Good |
| WASI Support | Full | Full | Full |
| Memory Safety | ByteCode Alliance | MIT | MIT |
| License | Apache 2.0 | MIT | MIT |
| Compilation | Cranelift, Singlepass, Cranelift | Cranelift, LLVM, Singlepass | Singlepass |
| Module caching | Yes (ModuleCache) | Yes | Limited |
| Fuel/Metering | Yes | Yes | Yes |
| Streaming compile | Yes | Yes | No |

### Decision: Wasmtime

**Chosen**: Wasmtime

**Rationale**:
- Developed by ByteCode Alliance (Mozilla, Fastly, Intel, Red Hat)
- Best Rust embedding API (`wasmtime` crate)
- Excellent WASI support
- Built-in fuel/metering for execution limits
- Module caching for fast reload
- Active development, production-proven
- Used by Fermyon, Fastly, and other production systems

**Trade-offs**:
- Slightly larger binary size than Wasmer Singlepass
- Cranelift compilation is slower than Singlepass (mitigated by module caching)

## Wasmtime Integration

```rust
use wasmtime::*;

pub struct WasmRuntime {
    engine: Engine,
    linker: Linker<WasmState>,
    module_cache: ModuleCache,
}

pub struct WasmState {
    plugin_name: String,
    capabilities: PluginCapabilities,
    host_functions: HostFunctions,
    memory_limit: usize,
}

impl WasmRuntime {
    pub fn new(config: &PluginConfig) -> Result<Self> {
        let mut engine_config = Config::new();
        engine_config
            .strategy(Strategy::Cranelift)
            .consume_fuel(true)
            .epoch_interruption(true)
            .memory_init_cow(true)
            .parallel_compilation(true);

        let engine = Engine::new(&engine_config)?;

        let mut linker = Linker::new(&engine);
        Self::add_host_functions(&mut linker)?;

        let module_cache = ModuleCache::new(
            config.cache_dir.as_ref(),
            &engine,
        )?;

        Ok(Self {
            engine,
            linker,
            module_cache,
        })
    }

    pub async fn instantiate(
        &self,
        wasm_bytes: &[u8],
        state: WasmState,
        memory_limit: usize,
    ) -> Result<WasmInstance> {
        // Compile or load from cache
        let module = self.module_cache.get_or_compile(wasm_bytes).await?;

        // Set fuel for execution limits
        let engine = self.engine.clone();
        let store = Store::new(&engine, state);
        store.limiter(|s| s.memory_limit as u64);
        store.set_fuel(1_000_000)?;  // 1M fuel units

        // Create instance with linker
        let instance = self.linker.instantiate(&mut store, &module)?;

        Ok(WasmInstance { store, instance })
    }

    fn add_host_functions(linker: &mut Linker<WasmState>) -> Result<()> {
        // Logging
        linker.func_wrap("host", "log",
            |caller: Caller<'_, WasmState>, level: u32, ptr: i32, len: i32| {
                let memory = caller.data().host_functions.get_memory(&caller);
                let message = read_string_from_memory(&memory, ptr, len);
                match level {
                    0 => tracing::trace!("{}", message),
                    1 => tracing::debug!("{}", message),
                    2 => tracing::info!("{}", message),
                    3 => tracing::warn!("{}", message),
                    _ => tracing::error!("{}", message),
                }
            }
        )?;

        // Storage
        linker.func_wrap("host", "storage_get",
            |mut caller: Caller<'_, WasmState>, key_ptr: i32, key_len: i32| -> i64 {
                let memory = caller.data().host_functions.get_memory(&caller);
                let key = read_string_from_memory(&memory, key_ptr, key_len);
                let value = caller.data().host_functions.storage_get(&key);
                write_buffer_to_memory(&mut caller, &value)
            }
        )?;

        // Events
        linker.func_wrap("host", "emit_event",
            |mut caller: Caller<'_, WasmState>,
             type_ptr: i32, type_len: i32,
             data_ptr: i32, data_len: i32| {
                let memory = caller.data().host_functions.get_memory(&caller);
                let event_type = read_string_from_memory(&memory, type_ptr, type_len);
                let data = read_bytes_from_memory(&memory, data_ptr, data_len);
                caller.data().host_functions.emit_event(&event_type, &data);
            }
        )?;

        // Timer
        linker.func_wrap("host", "set_timer",
            |mut caller: Caller<'_, WasmState>, interval_ms: u64, callback_id: u32| -> u64 {
                caller.data().host_functions.set_timer(interval_ms, callback_id)
            }
        )?;

        // Player operations
        linker.func_wrap("host", "player_get",
            |mut caller: Caller<'_, WasmState>, player_id: u64| -> i64 {
                let data = caller.data().host_functions.player_get(player_id);
                write_buffer_to_memory(&mut caller, &data)
            }
        )?;

        // Inventory operations
        linker.func_wrap("host", "inventory_add_item",
            |mut caller: Caller<'_, WasmState>,
             player_id: u64, item_id: u32, quantity: u32| -> i32 {
                caller.data().host_functions.inventory_add_item(player_id, item_id, quantity)
            }
        )?;

        Ok(())
    }
}
```

## Fuel Metering (Execution Limits)

```rust
impl WasmInstance {
    pub fn set_execution_limit(&mut self, fuel: u64) -> Result<()> {
        self.store.set_fuel(fuel)
    }

    pub fn consume_fuel(&mut self, amount: u64) -> Result<u64> {
        self.store.consume_fuel(amount)
    }

    pub async fn call_with_timeout<F, T>(
        &mut self,
        timeout: Duration,
        f: F,
    ) -> Result<T>
    where
        F: FnOnce(&mut Store<WasmState>, &Instance) -> Result<T>,
    {
        tokio::time::timeout(timeout, async {
            self.store.set_fuel(1_000_000)?;
            f(&mut self.store, &self.instance)
        })
        .await
        .map_err(|_| WasmError::ExecutionTimeout)?
    }
}
```

## Memory Limits

```rust
impl WasmState {
    pub fn memory_usage(&self) -> usize {
        // Track memory allocations through host functions
        self.memory_usage
    }

    pub fn check_memory_limit(&self) -> Result<()> {
        if self.memory_usage > self.capabilities.memory_limit {
            return Err(WasmError::MemoryLimitExceeded {
                current: self.memory_usage,
                limit: self.capabilities.memory_limit,
            });
        }
        Ok(())
    }
}

// Wasmtime limiter integration
fn memory_limiter(state: &mut WasmState) -> impl MemoryCreator + '_ {
    MemoryLimiter {
        remaining: state.capabilities.memory_limit as i64,
    }
}

struct MemoryLimiter {
    remaining: i64,
}

impl MemoryCreator for MemoryLimiter {
    fn new_memory(
        &mut self,
        byte_length: usize,
        _initial: usize,
        _maximum: Option<usize>,
        _shared: bool,
    ) -> Result<Memory> {
        if byte_length as i64 > self.remaining {
            return Err(MemoryError::OutOfMemory);
        }
        self.remaining -= byte_length as i64;
        Memory::new(byte_length, maximum)
    }
}
```

## Module Caching

```rust
pub struct ModuleCache {
    cache_dir: PathBuf,
    engine: Engine,
}

impl ModuleCache {
    pub fn new(cache_dir: Option<&PathBuf>, engine: &Engine) -> Result<Self> {
        if let Some(dir) = cache_dir {
            fs::create_dir_all(dir)?;
        }
        Ok(Self {
            cache_dir: cache_dir.cloned().unwrap_or_default(),
            engine: engine.clone(),
        })
    }

    pub async fn get_or_compile(&self, wasm_bytes: &[u8]) -> Result<Module> {
        let hash = blake3::hash(wasm_bytes);
        let cache_path = self.cache_dir.join(format!("{}.bin", hash));

        if cache_path.exists() {
            let cached = fs::read(&cache_path).await?;
            return Ok(unsafe { Module::deserialize(&self.engine, &cached)? });
        }

        let module = Module::new(&self.engine, wasm_bytes)?;
        let serialized = module.serialize()?;
        fs::write(&cache_path, serialized).await?;
        Ok(module)
    }
}
```

## WASI Integration

Plugins run in a WASI environment for file system access (sandboxed):

```rust
fn configure_wasi(state: &WasmState) -> Result<WasiCtx> {
    let mut wasi = WasiCtxBuilder::new();

    // Sandboxed: plugin gets its own directory
    let plugin_dir = PathBuf::from("plugins").join(&state.plugin_name);
    wasi.preopened_dir(plugin_dir, "/data")?;

    // Environment variables (read-only)
    wasi.env("PLUGIN_NAME", &state.plugin_name);
    wasi.env("PLUGIN_VERSION", &state.manifest.version);

    // Stdio for logging
    wasi.inherit_stdout();
    wasi.inherit_stderr();

    Ok(wasi.build()?)
}
```

## Thread Safety

```rust
// Each plugin instance gets its own Store (thread-safe)
// Multiple plugins can run concurrently
// Wasmtime's Store is !Send, so we use one thread per plugin instance

pub struct PluginThreadPool {
    handles: Vec<JoinHandle<()>>,
    command_tx: mpsc::Sender<PluginCommand>,
}

impl PluginThreadPool {
    pub fn new(size: usize) -> Self {
        let (tx, rx) = mpsc::channel(100);
        let rx = Arc::new(Mutex::new(rx));

        let handles = (0..size).map(|_| {
            let rx = rx.clone();
            std::thread::spawn(move || {
                Self::worker_loop(rx);
            })
        }).collect();

        Self { handles, command_tx: tx }
    }
}
```

## Security Considerations

| Threat | Mitigation |
|--------|-----------|
| Memory exhaustion | Per-plugin memory limits via WASM linear memory |
| CPU exhaustion | Fuel metering + execution timeouts |
| File system access | WASI sandbox with preopened directories only |
| Network access | Capability-checked host functions only |
| Unsafe WASM | Wasmtime validates all WASM modules |
| Supply chain | Plugin signing + manifest verification (future) |

## Performance Optimization

```rust
// 1. Parallel compilation
engine_config.parallel_compilation(true);

// 2. Module caching (avoid recompilation)
let module = module_cache.get_or_compile(wasm_bytes).await?;

// 3. Epoch-based interruption (for long-running plugins)
engine_config.epoch_interruption(true);

// 4. Memory init COW (faster instantiation)
engine_config.memory_init_cow(true);

// 5. Pool pre-compiled modules
let module_pool = ModulePool::new(&engine, module_count).await?;
```

## References

- [04-plugin-system.md](04-plugin-system.md) - Plugin lifecycle
- [05-plugin-api.md](05-plugin-api.md) - Plugin API contract
- [03-capability.md](03-capability.md) - Capability enforcement
