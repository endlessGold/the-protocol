# 05 - Plugin API

## Overview

The Plugin API defines the contract between the Runtime and WASM plugins. Plugins call host functions provided by the Runtime, and the Runtime calls exported functions from plugins.

## Host Functions (Runtime → Plugin)

These are functions the Runtime provides to plugins via WASM imports.

### Core Host Functions

```rust
// Logging
fn log(level: u32, message_ptr: *const u8, message_len: usize);

// Storage
fn storage_get(key_ptr: *const u8, key_len: usize) -> i64;  // returns buffer handle
fn storage_set(key_ptr: *const u8, key_len: usize, value_ptr: *const u8, value_len: usize) -> i32;
fn storage_delete(key_ptr: *const u8, key_len: usize) -> i32;

// Events
fn emit_event(event_type_ptr: *const u8, event_type_len: usize,
              data_ptr: *const u8, data_len: usize);

// Timers
fn set_timer(interval_ms: u64, callback_id: u32) -> u64;  // returns timer handle
fn cancel_timer(handle: u64);

// Scheduling
fn schedule_task(delay_ms: u64, callback_id: u32) -> u64;

// HTTP (if network capability enabled)
fn http_request(method_ptr: *const u8, method_len: usize,
                url_ptr: *const u8, url_len: usize,
                body_ptr: *const u8, body_len: usize,
                callback_id: u32) -> u64;
```

### Domain Host Functions

```rust
// Player operations
fn player_get(player_id: u64) -> i64;           // returns buffer handle
fn player_update(player_id: u64, data_ptr: *const u8, data_len: usize) -> i32;

// Inventory operations
fn inventory_get(player_id: u64) -> i64;
fn inventory_add_item(player_id: u64, item_id: u32, quantity: u32) -> i32;
fn inventory_remove_item(player_id: u64, item_id: u32, quantity: u32) -> i32;

// World operations
fn world_get_room(room_id: u32) -> i64;
fn world_move_entity(entity_id: u64, room_id: u32) -> i32;

// Combat operations
fn combat_start(attacker_id: u64, target_id: u64) -> i64;
fn combat_action(combat_id: u64, action_ptr: *const u8, action_len: usize) -> i64;
```

### Communication Host Functions

```rust
// Send message to client
fn send_to_client(session_id: u64, data_ptr: *const u8, data_len: usize);

// Broadcast to all clients in room
fn broadcast_to_room(room_id: u32, data_ptr: *const u8, data_len: usize);

// Send to another plugin
fn send_to_plugin(target_plugin: *const u8, target_len: usize,
                  data_ptr: *const u8, data_len: usize);
```

## Plugin Exports (Plugin → Runtime)

These are functions plugins export for the Runtime to call.

### Lifecycle Exports

```rust
// Called when plugin is loaded
fn plugin_init() -> i32;  // 0 = success

// Called when plugin is enabled
fn plugin_enable() -> i32;

// Called when plugin is disabled
fn plugin_disable() -> i32;

// Called before plugin is unloaded
fn plugin_unload();
```

### Command Handler Export

```rust
// Handle a command from client
// command_type: u32, data_ptr/len: command payload
// Returns: response buffer handle
fn handle_command(command_type: u32, data_ptr: *const u8, data_len: usize) -> i64;
```

### Event Handler Export

```rust
// Handle an event from the event bus
fn handle_event(event_type: u32, data_ptr: *const u8, data_len: usize);
```

### Timer Callback Export

```rust
// Handle timer callback
fn handle_timer(callback_id: u32, timer_handle: u64);
```

## Message Format

All plugin ↔ runtime communication uses MessagePack serialization.

### Command Message

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct CommandMessage {
    pub id: u64,
    pub command_type: String,  // "attack", "move", "look", etc.
    pub sender_id: u64,        // player session ID
    pub timestamp: u64,
    pub payload: Vec<u8>,      // MessagePack-encoded command data
}
```

### Event Message

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct EventMessage {
    pub id: u64,
    pub event_type: String,    // "attack_executed", "item_acquired", etc.
    pub source_plugin: String,
    pub timestamp: u64,
    pub payload: Vec<u8>,
    pub targets: Option<Vec<u64>>,  // None = broadcast
}
```

## Plugin Registration API

Plugins register their capabilities during initialization:

```rust
// Plugin calls this during init to register commands
fn register_command(name_ptr: *const u8, name_len: usize, handler_id: u32);

// Plugin calls this to register event handlers
fn register_event_handler(event_type_ptr: *const u8, event_type_len: usize, handler_id: u32);

// Plugin calls this to register routes (for HTTP-capable plugins)
fn register_route(method_ptr: *const u8, method_len: usize,
                  path_ptr: *const u8, path_len: usize, handler_id: u32);
```

## Memory Management

Plugins and Runtime share memory through a simple buffer protocol:

```
1. Host allocates buffer in plugin memory
2. Host writes data to buffer
3. Host calls plugin function with buffer handle
4. Plugin reads data from buffer
5. Plugin frees buffer when done

Buffer handles are i64 values:
  - Positive: valid buffer handle
  - 0: null/empty
  - Negative: error code
```

```rust
// Host function to allocate buffer in plugin memory
fn allocate_buffer(size: usize) -> i64;  // returns handle

// Host function to read from buffer
fn read_buffer(handle: i64, offset: usize, dest_ptr: *mut u8, len: usize) -> i32;

// Host function to free buffer
fn free_buffer(handle: i64);
```

## TypeScript Example

```typescript
// Plugin entry point (compiled to WASM)
import { registerCommand, registerEventHandler, emitEvent } from "@protocol/sdk";

// Called when plugin initializes
export function plugin_init(): number {
    registerCommand("attack", handleAttack);
    registerCommand("defend", handleDefend);
    registerEventHandler("combat_started", onCombatStarted);
    return 0; // success
}

function handleAttack(command: CommandMessage): ResponseMessage {
    const data = decodePayload<AttackCommand>(command.payload);

    // Validate attack
    if (!canAttack(command.sender_id, data.target_id)) {
        return { success: false, error: "Cannot attack target" };
    }

    // Emit combat event
    emitEvent("attack_executed", {
        attacker: command.sender_id,
        target: data.target_id,
        weapon: data.weapon_id,
    });

    return { success: true };
}
```

## API Versioning

```rust
pub const PLUGIN_API_VERSION: &str = "1.0";

// Version compatibility rules:
// MAJOR.MINOR
// - Same MAJOR: backward compatible
// - Different MAJOR: incompatible
// - Higher MINOR: new optional features
```

| API Version | Runtime Version | Compatibility |
|-------------|-----------------|---------------|
| 1.0 | 1.x.x | Compatible |
| 1.1 | 1.0.x | Plugin uses features not in Runtime → graceful fallback |
| 2.0 | 1.x.x | Incompatible → reject plugin |
| 1.0 | 2.x.x | Compatible → Runtime provides backward compat |

## References

- [04-plugin-system.md](04-plugin-system.md) - Plugin lifecycle
- [06-wasm.md](06-wasm.md) - WASM implementation
- [03-capability.md](03-capability.md) - Permission checking
