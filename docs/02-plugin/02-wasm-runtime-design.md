# WASM 런타임 통합 설계

> The Protocol WASM 런타임 아키텍처 및 Wasmtime 통합 상세 설계

## 1. 개요

The Protocol은 WASM 플러그인을 위한 런타임으로 Wasmtime을 선택합니다. 이 문서는 Wasmtime 통합 아키텍처, Host Function 인터페이스, 메모리 관리, 성능 최적화 전략을 다룹니다.

## 2. Wasmtime vs Wasmer 비교 분석

### 2.1 비교 매트릭스

| 항목 | Wasmtime | Wasmer |
|------|----------|--------|
| **개발 주체** | Bytecode Alliance | Wasmer, Inc. |
| **언어** | Rust | Rust |
| **WASI 지원** | 완전 지원 (WASI Preview 1, 2) | 부분 지원 |
| **Cranelift 백엔드** | ✅ 기본 | ❌ |
| **LLVM 백엔드** | ❌ | ✅ |
| **Singlepass 백엔드** | ❌ | ✅ |
| **Fuel Metering** | ✅ 네이티브 지원 | ✅ 네이티브 지원 |
| **WASM 상태 저장** | ✅ (사전 컴파일 캐시) | ✅ (κρατήσεις) |
| **API 안정성** | ✅ 안정적 | ⚠️ 변경 잦음 |
| **메모리 안전성** | ✅ 강화 | ✅ 양호 |
| **Community** | ✅ 활발 | ✅ 활발 |
| **Rust API** | ✅ 우수 | ✅ 양호 |
| **嶄新 기능** | Component Model | Jones, Singlepass |

### 2.2 성능 비교

```
Benchmarks (relative):
──────────────────────────────────────────────────
                      Wasmtime    Wasmer
──────────────────────────────────────────────────
Compile Time         ████░░░░    ██░░░░░░  (Wasmtime 느림)
Execute Speed        ████████    ████████  (유사)
Memory Usage         ██████░░    █████░░░  (Wasmtime 약간 높음)
WASI Compliance      ████████    ██████░░  (Wasmtime 우수)
──────────────────────────────────────────────────
```

### 2.3 선택 이유: Wasmtime

1. **WASI 완전 지원**: WASI Preview 1, 2 모두 지원. The Protocol의 플러그인 시스템은 WASI 의존성이 높음
2. **API 안정성**: Bytecode Alliance의 강한 API 안정성 보장. 장기 프로젝트에 적합
3. **Fuel Metering**: 네이티브 지원으로 플러그인 실행 제한 구현 용이
4. **상태 저장**: 사전 컴파일된 모듈 캐싱으로 로딩 성능 향상
5. **보안**: Bytecode Alliance의 보안 중심 개발 방식
6. **Component Model**: 향후 WASM 컴포넌트 모델 지원 예정

## 3. Engine/Store/Module/Instance 관계

```
┌─────────────────────────────────────────────────────┐
│                    Wasmtime Engine                    │
│  ┌───────────────────────────────────────────────┐  │
│  │              Engine Configuration             │  │
│  │  - Fuel enabled: true                         │  │
│  │  - Max instances: 100                         │  │
│  │  - Interrupt strategy: fuel                   │  │
│  └───────────────────────────────────────────────┘  │
│                                                      │
│  ┌──────────────┐  ┌──────────────┐                │
│  │  Module A     │  │  Module B     │  (컴파일된 모듈) │
│  │  (컴파일 캐시) │  │  (컴파일 캐시) │                │
│  └──────────────┘  └──────────────┘                │
│                                                      │
│  ┌──────────────┐  ┌──────────────┐                │
│  │   Store A     │  │   Store B     │  (격리된 상태)  │
│  │  ┌──────────┐│  │  ┌──────────┐│                │
│  │  │Instance A││  │  │Instance B││  (플러그인 인스턴스)│
│  │  │ Memory   ││  │  │ Memory   ││                │
│  │  │ Table    ││  │  │ Table    ││                │
│  │  │ Fuel     ││  │  │ Fuel     ││                │
│  │  └──────────┘│  │  └──────────┘│                │
│  └──────────────┘  └──────────────┘                │
└─────────────────────────────────────────────────────┘
```

### 3.1 Engine

- Wasmtime 엔진의 전역 설정 관리
- 컴파일러 설정 (Cranelift 옵션)
- 메모리 설정 (Linear Memory Growing Strategy)
- 전역 리소스 제한

```rust
pub struct PluginEngine {
    engine: wasmtime::Engine,
    module_cache: Arc<RwLock<HashMap<String, wasmtime::Module>>>,
}

impl PluginEngine {
    pub fn new() -> Self {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        config.epoch_interruption(true);
        config.memory_init_cow(true);
        config.memory_guaranteed_double_linked(true);

        let engine = wasmtime::Engine::new(&config).expect("Failed to create engine");

        Self {
            engine,
            module_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
```

### 3.2 Store

- 플러그인별 독립된 상태 보유
- Fuel, Memory 제한 관리
- Host Data (런타임 데이터) 포함

```rust
pub struct PluginStore {
    store: wasmtime::Store<PluginState>,
}

pub struct PluginState {
    pub plugin_name: String,
    pub fuel_limit: u64,
    pub memory_limit: usize,
    pub permissions: Vec<String>,
    pub host_context: HostContext,
}

pub struct HostContext {
    pub storage: Box<dyn StorageProvider>,
    pub event_bus: Arc<EventBus>,
    pub logger: Arc<dyn Logger>,
}
```

### 3.3 Module

- 컴파일된 WASM 모듈
- 재사용 가능 (동일 모듈 → 다수 인스턴스)
- 파일 시스템 또는 메모리 캐시

### 3.4 Instance

- 모듈의 인스턴스화된 실행 단위
- 독립된 Linear Memory
- Export된 함수 호출 가능
- Host Function을 통해 런타임과 통신

## 4. WASI 설정

### 4.1 WASI 컨텍스트 구성

```rust
pub fn create_wasi_context(plugin_name: &str) -> wasmtime::WasiCtx {
    wasmtime::WasiCtxBuilder::new()
        .arg(plugin_name)
        .env("PLUGIN_NAME", plugin_name)
        .env("PLUGIN_VERSION", "1.0.0")
        .env("API_VERSION", "1.0")
        .preopened_dir(
            plugin_data_dir(plugin_name),
            "/data",
        )
        .preopened_dir(
            plugin_config_dir(plugin_name),
            "/config",
        )
        .inherit_stdio()
        .build()
}
```

### 4.2 Filesystem 접근

```
/plugin-data/
├── {plugin-name}/
│   ├── plugin.toml        # 매니페스트 (읽기 전용)
│   ├── plugin.wasm        # WASM 모듈 (읽기 전용)
│   ├── data/              # 플러그인 데이터 디렉토리
│   │   └── ...            # 플러그인별 데이터 저장
│   └── config/            # 플러그인 설정 디렉토리
│       └── ...
```

### 4.3 환경 변수

| 변수 | 설명 | 예시 |
|------|------|------|
| `PLUGIN_NAME` | 플러그인 이름 | `combat-system` |
| `PLUGIN_VERSION` | 플러그인 버전 | `1.0.0` |
| `API_VERSION` | 호환 API 버전 | `1.0` |
| `SERVER_VERSION` | 서버 버전 | `0.5.0` |

## 5. Host Function 인터페이스 전체 명세

### 5.1 Logging

```rust
// log(level: i32, ptr: *const u8, len: usize)
#[wasmtime::func]
pub fn log(store: &mut Store<PluginState>, level: i32, ptr: u32, len: u32) {
    let memory = store.data().memory.data(&store);
    let message = std::str::from_utf8(&memory[ptr as usize..(ptr + len) as usize])
        .unwrap_or("[invalid utf-8]");

    match level {
        0 => tracing::trace!("[{}] {}", store.data().plugin_name, message),
        1 => tracing::debug!("[{}] {}", store.data().plugin_name, message),
        2 => tracing::info!("[{}] {}", store.data().plugin_name, message),
        3 => tracing::warn!("[{}] {}", store.data().plugin_name, message),
        4 => tracing::error!("[{}] {}", store.data().plugin_name, message),
        _ => tracing::info!("[{}] {}", store.data().plugin_name, message),
    }
}
```

### 5.2 Storage

```rust
// storage_get(key_ptr: *const u8, key_len: usize) -> i64
// Returns: buffer_id (positive) or error code (negative)
#[wasmtime::func]
pub fn storage_get(store: &mut Store<PluginState>, key_ptr: u32, key_len: u32) -> i64 {
    let memory = store.data().memory.data(&store);
    let key = match std::str::from_utf8(&memory[key_ptr as usize..(key_ptr + key_len) as usize]) {
        Ok(k) => k,
        Err(_) => return -1, // INVALID_KEY
    };

    match store.data().host_context.storage.get(&store.data().plugin_name, key) {
        Some(value) => {
            let buffer_id = allocate_buffer(store, &value);
            buffer_id as i64
        }
        None => -2, // NOT_FOUND
    }
}

// storage_set(key_ptr: *const u8, key_len: usize, val_ptr: *const u8, val_len: usize) -> i32
#[wasmtime::func]
pub fn storage_set(store: &mut Store<PluginState>, key_ptr: u32, key_len: u32, val_ptr: u32, val_len: u32) -> i32 {
    let memory = store.data().memory.data(&store);
    let key = match std::str::from_utf8(&memory[key_ptr as usize..(key_ptr + key_len) as usize]) {
        Ok(k) => k,
        Err(_) => return -1,
    };
    let value = &memory[val_ptr as usize..(val_ptr + val_len) as usize];

    match store.data().host_context.storage.set(&store.data().plugin_name, key, value) {
        Ok(_) => 0,  // SUCCESS
        Err(_) => -3, // STORAGE_ERROR
    }
}

// storage_delete(key_ptr: *const u8, key_len: usize) -> i32
#[wasmtime::func]
pub fn storage_delete(store: &mut Store<PluginState>, key_ptr: u32, key_len: u32) -> i32 {
    let memory = store.data().memory.data(&store);
    let key = match std::str::from_utf8(&memory[key_ptr as usize..(key_ptr + key_len) as usize]) {
        Ok(k) => k,
        Err(_) => return -1,
    };

    match store.data().host_context.storage.delete(&store.data().plugin_name, key) {
        Ok(_) => 0,
        Err(_) => -2,
    }
}
```

### 5.3 Events

```rust
// emit_event(event_type_ptr: *const u8, event_type_len: usize,
//            data_ptr: *const u8, data_len: usize) -> i32
#[wasmtime::func]
pub fn emit_event(
    store: &mut Store<PluginState>,
    event_type_ptr: u32, event_type_len: u32,
    data_ptr: u32, data_len: u32,
) -> i32 {
    let memory = store.data().memory.data(&store);
    let event_type = match std::str::from_utf8(
        &memory[event_type_ptr as usize..(event_type_ptr + event_type_len) as usize]
    ) {
        Ok(t) => t,
        Err(_) => return -1,
    };
    let data = &memory[data_ptr as usize..(data_ptr + data_len) as usize];

    let event = PluginEvent {
        source: store.data().plugin_name.clone(),
        event_type: event_type.to_string(),
        data: data.to_vec(),
        timestamp: chrono::Utc::now(),
    };

    store.data().host_context.event_bus.emit(event);
    0
}
```

### 5.4 Timers

```rust
// set_timer(delay_ms: i64, repeat: i32, callback_id: i32) -> i64
// Returns: timer_id or error code
#[wasmtime::func]
pub fn set_timer(store: &mut Store<PluginState>, delay_ms: i64, repeat: i32, callback_id: i32) -> i64 {
    let timer = Timer {
        plugin_name: store.data().plugin_name.clone(),
        delay_ms,
        repeat: repeat != 0,
        callback_id,
    };

    match store.data().host_context.timer_registry.register(timer) {
        Ok(id) => id as i64,
        Err(_) => -1,
    }
}

// cancel_timer(timer_id: i64) -> i32
#[wasmtime::func]
pub fn cancel_timer(store: &mut Store<PluginState>, timer_id: i64) -> i32 {
    match store.data().host_context.timer_registry.cancel(timer_id) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}
```

### 5.5 Player Operations

```rust
// player_get(player_id: i64) -> i64
// Returns: buffer_id with MessagePack encoded PlayerData
#[wasmtime::func]
pub fn player_get(store: &mut Store<PluginState>, player_id: i64) -> i64 {
    match store.data().host_context.world.get_player(player_id) {
        Some(player) => {
            let data = rmp_serde::to_vec(&player).unwrap_or_default();
            allocate_buffer(store, &data) as i64
        }
        None => -2, // NOT_FOUND
    }
}

// player_update(player_id: i64, data_ptr: *const u8, data_len: usize) -> i32
#[wasmtime::func]
pub fn player_update(store: &mut Store<PluginState>, player_id: i64, data_ptr: u32, data_len: u32) -> i32 {
    let memory = store.data().memory.data(&store);
    let data = &memory[data_ptr as usize..(data_ptr + data_len) as usize];

    match rmp_serde::from_slice::<PlayerUpdate>(data) {
        Ok(update) => {
            store.data().host_context.world.update_player(player_id, update);
            0
        }
        Err(_) => -1, // DESERIALIZATION_ERROR
    }
}
```

### 5.6 Inventory Operations

```rust
// inventory_get(player_id: i64) -> i64
#[wasmtime::func]
pub fn inventory_get(store: &mut Store<PluginState>, player_id: i64) -> i64 {
    match store.data().host_context.world.get_inventory(player_id) {
        Some(inv) => {
            let data = rmp_serde::to_vec(&inv).unwrap_or_default();
            allocate_buffer(store, &data) as i64
        }
        None => -2,
    }
}

// inventory_add_item(player_id: i64, item_id: i64, count: i32) -> i32
#[wasmtime::func]
pub fn inventory_add_item(store: &mut Store<PluginState>, player_id: i64, item_id: i64, count: i32) -> i32 {
    store.data().host_context.world.add_item(player_id, item_id, count);
    0
}

// inventory_remove_item(player_id: i64, item_id: i64, count: i32) -> i32
#[wasmtime::func]
pub fn inventory_remove_item(store: &mut Store<PluginState>, player_id: i64, item_id: i64, count: i32) -> i32 {
    match store.data().host_context.world.remove_item(player_id, item_id, count) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}
```

### 5.7 Combat Operations

```rust
// combat_start(attacker_id: i64, defender_id: i64) -> i64
// Returns: combat_id
#[wasmtime::func]
pub fn combat_start(store: &mut Store<PluginState>, attacker_id: i64, defender_id: i64) -> i64 {
    match store.data().host_context.combat.start(attacker_id, defender_id) {
        Ok(combat_id) => combat_id as i64,
        Err(_) => -1,
    }
}

// combat_action(combat_id: i64, action_ptr: *const u8, action_len: usize) -> i32
#[wasmtime::func]
pub fn combat_action(store: &mut Store<PluginState>, combat_id: i64, action_ptr: u32, action_len: u32) -> i32 {
    let memory = store.data().memory.data(&store);
    let action_data = &memory[action_ptr as usize..(action_ptr + action_len) as usize];

    match rmp_serde::from_slice::<CombatAction>(action_data) {
        Ok(action) => {
            store.data().host_context.combat.execute(combat_id, action);
            0
        }
        Err(_) => -1,
    }
}
```

### 5.8 Communication

```rust
// send_to_client(player_id: i64, msg_ptr: *const u8, msg_len: usize) -> i32
#[wasmtime::func]
pub fn send_to_client(store: &mut Store<PluginState>, player_id: i64, msg_ptr: u32, msg_len: u32) -> i32 {
    let memory = store.data().memory.data(&store);
    let message = &memory[msg_ptr as usize..(msg_ptr + msg_len) as usize];

    store.data().host_context.network.send_to_player(player_id, message);
    0
}

// broadcast_to_room(room_id: i64, msg_ptr: *const u8, msg_len: usize) -> i32
#[wasmtime::func]
pub fn broadcast_to_room(store: &mut Store<PluginState>, room_id: i64, msg_ptr: u32, msg_len: u32) -> i32 {
    let memory = store.data().memory.data(&store);
    let message = &memory[msg_ptr as usize..(msg_ptr + msg_len) as usize];

    store.data().host_context.network.broadcast(room_id, message);
    0
}
```

## 6. 메모리 관리

### 6.1 Linear Memory

- 각 플러그인 인스턴스는 독립된 Linear Memory 보유
- 초기 페이지 수: 256 (16MB)
- 최대 페이지 수: 4096 (256MB)
- 페이지 크기: 64KB

### 6.2 Buffer Protocol

```
┌─────────────────────────────────────────────┐
│            Host Buffer Management            │
│                                              │
│  Plugin A ←→ Buffer Pool ←→ Plugin B        │
│                                              │
│  allocate_buffer(data) -> buffer_id          │
│  read_buffer(buffer_id) -> data             │
│  free_buffer(buffer_id)                      │
└─────────────────────────────────────────────┘
```

```rust
pub struct BufferPool {
    buffers: HashMap<i64, Vec<u8>>,
    next_id: i64,
}

impl BufferPool {
    pub fn allocate(&mut self, data: &[u8]) -> i64 {
        let id = self.next_id;
        self.buffers.insert(id, data.to_vec());
        self.next_id += 1;
        id
    }

    pub fn read(&self, id: i64) -> Option<&[u8]> {
        self.buffers.get(&id).map(|v| v.as_slice())
    }

    pub fn free(&mut self, id: i64) {
        self.buffers.remove(&id);
    }
}
```

### 6.3 데이터 교환 흐름

```
Host → Plugin:
1. Host가 데이터를 BufferPool에 저장
2. buffer_id를 플러그인에 전달
3. 플러그인이 read_buffer(buffer_id)로 데이터 읽기
4. 플러그인이 free_buffer(buffer_id)로 해제

Plugin → Host:
1. 플러그인이 allocate_buffer(size)로 버퍼 할당
2. 플러그인이 write_buffer(buffer_id, data)로 데이터 기록
3. Host가 read_buffer(buffer_id)로 데이터 읽기
4. Host가 free_buffer(buffer_id)로 해제
```

## 7. Fuel Metering (실행 제한)

### 7.1 설정

```rust
pub fn configure_fuel(store: &mut Store<PluginState>, fuel_limit: u64) {
    store.set_fuel(fuel_limit).expect("Failed to set fuel");
}

pub fn consume_fuel(store: &mut Store<PluginState>, amount: u64) -> Result<u64, Trap> {
    store.consume_fuel(amount)
}
```

### 7.2 Fuel 소비 예시

| 연산 | Fuel 비용 |
|------|----------|
| 기본 연산 (add, sub, mul) | 1 |
| 비교 연산 | 1 |
| 메모리 접근 | 10 |
| 함수 호출 | 100 |
| Host Function 호출 | 1,000 |
| 메모리 할당 | 10,000 |
| 문자열 처리 | 50 |

### 7.3 Fuel 초과 처리

- Fuel 0 도달 시 `Trap::OutOfFuel` 발생
- 플러그인 함수 실행 자동 중단
- 에러 로깅 후 플러그인 비활성화 검토

## 8. Memory Limit (메모리 제한)

### 8.1 설정

```rust
pub struct MemoryConfig {
    pub initial_pages: u32,    // 256 (16MB)
    pub maximum_pages: u32,    // 4096 (256MB)
    pub memory_limit: usize,   // bytes
}
```

### 8.2 메모리 사용량 추적

```rust
pub fn get_memory_usage(store: &Store<PluginState>) -> usize {
    store.data().memory.data(store).len()
}

pub fn check_memory_limit(store: &Store<PluginState>) -> bool {
    get_memory_usage(store) <= store.data().memory_limit
}
```

## 9. Module Caching

### 9.1 캐시 전략

```
┌─────────────────────────────────────────────┐
│              Module Cache                     │
│                                              │
│  File System Cache:                          │
│  .cache/plugins/                             │
│  ├── {plugin-name}-{version}.wasm.cache      │
│  └── ...                                     │
│                                              │
│  Memory Cache (LRU):                         │
│  ┌─────────────────────────────────────┐    │
│  │ Module A │ Module B │ Module C │    │    │
│  │ (recent) │ (recent) │ (recent) │    │    │
│  └─────────────────────────────────────┘    │
└─────────────────────────────────────────────┘
```

```rust
pub struct ModuleCache {
    memory_cache: LruCache<String, wasmtime::Module>,
    cache_dir: PathBuf,
}

impl ModuleCache {
    pub fn get_or_compile(
        &mut self,
        engine: &wasmtime::Engine,
        plugin_name: &str,
        wasm_bytes: &[u8],
    ) -> Result<wasmtime::Module, wasmtime::Error> {
        let cache_key = format!("{}-{}", plugin_name, blake3::hash(wasm_bytes));

        // 메모리 캐시 확인
        if let Some(module) = self.memory_cache.get(&cache_key) {
            return Ok(module.clone());
        }

        // 파일 시스템 캐시 확인
        let cache_path = self.cache_dir.join(format!("{}.wasm.cache", cache_key));
        if cache_path.exists() {
            let module = unsafe {
                wasmtime::Module::from_file(engine, &cache_path)?
            };
            self.memory_cache.put(cache_key, module.clone());
            return Ok(module);
        }

        // 새 모듈 컴파일
        let module = wasmtime::Module::new(engine, wasm_bytes)?;

        // 캐시에 저장
        module.serialize_to_file(&cache_path)?;
        self.memory_cache.put(cache_key, module.clone());

        Ok(module)
    }
}
```

## 10. Thread Safety

### 10.1 Wasmtime 제약사항

- `wasmtime::Store`는 `!Send` (스레드 간 이동 불가)
- `wasmtime::Instance`는 `!Send`
- `wasmtime::Module`은 `Send + Sync` (스레드 간 공유 가능)

### 10.2 스레드 안전한 설계

```
┌─────────────────────────────────────────────┐
│           Thread Safety Model                │
│                                              │
│  Tokio Runtime (Multi-thread)                │
│  ┌──────────────────────────────────────┐   │
│  │ Thread 1: Plugin A Store            │   │
│  │ Thread 2: Plugin B Store            │   │
│  │ Thread 3: Plugin C Store            │   │
│  │ ...                                 │   │
│  └──────────────────────────────────────┘   │
│                                              │
│  Shared State (Arc + RwLock):               │
│  - Module Cache                            │
│  - Event Bus                               │
│  - Storage Provider                        │
│  - Player Data                             │
└─────────────────────────────────────────────┘
```

### 10.3 동기화 전략

```rust
pub struct PluginManager {
    // 각 플러그인은 자체 Store (Send 안됨)
    // 스레드 로컬 스토리지 또는 per-task 관리
    plugins: Arc<DashMap<String, PluginInstance>>,

    // 공유 상태
    module_cache: Arc<RwLock<ModuleCache>>,
    event_bus: Arc<EventBus>,
    storage: Arc<dyn StorageProvider>,
}

impl PluginManager {
    pub async fn execute_plugin<F, R>(
        &self,
        plugin_name: &str,
        f: F,
    ) -> Result<R, PluginError>
    where
        F: std::future::Future<Output = Result<R, PluginError>> + Send + 'static,
        R: Send + 'static,
    {
        // 각 플러그인 실행은 별도 태스크에서
        let plugin = self.plugins.get(plugin_name)
            .ok_or(PluginError::NotFound(plugin_name.to_string()))?;

        tokio::task::spawn_blocking(move || {
            // Store는 Send가 아니므로 블로킹 태스크에서 실행
            // WASM 함수 호출은 동기적
        }).await.map_err(|e| PluginError::Wasm(e.to_string()))?
    }
}
```

## 11. 성능 최적화 전략

### 11.1 사전 컴파일

- 서버 시작 시 모든 WASM 모듈 사전 컴파일
- 컴파일 결과를 파일 시스템에 캐시
- 캐시 히트 시 로딩 시간 90% 절감

### 11.2 메모리 풀링

- BufferPool 사전 할당으로 할당 오버헤드 감소
- 플러그인 종료 시 메모리 재사용

### 11.3 배치 처리

- 다수 플러그인의 이벤트 처리를 배치로 묶어 처리
- 컨텍스트 스위칭 오버헤드 감소

### 11.4 JIT 컴파일 최적화

- Cranelift 백엔드 최적화 레벨 설정
- 프로파일 기반 최적화 (PGO) 적용 가능

### 11.5 벤치마크

```
목표 성능:
- 플러그인 로딩: < 100ms (캐시 히트 시 < 10ms)
- 함수 호출 지연시간: < 1μs
- 이벤트 처리 throughput: > 10,000 events/sec
- 메모리 오버헤드: < 50MB (10개 플러그인 기준)
```
