use crate::error::PluginError;
use crate::state::HostState;
use wasmtime::Caller;

fn read_memory(
    caller: &mut Caller<'_, HostState>,
    ptr: u32,
    len: u32,
) -> Result<Vec<u8>, PluginError> {
    let memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| PluginError::Memory("memory export not found".into()))?;

    let mut buf = vec![0u8; len as usize];
    memory
        .read(&mut *caller, ptr as usize, &mut buf)
        .map_err(|e| PluginError::Memory(e.to_string()))?;
    Ok(buf)
}

fn write_memory(
    caller: &mut Caller<'_, HostState>,
    ptr: u32,
    data: &[u8],
) -> Result<(), PluginError> {
    let memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| PluginError::Memory("memory export not found".into()))?;

    memory
        .write(&mut *caller, ptr as usize, data)
        .map_err(|e| PluginError::Memory(e.to_string()))?;
    Ok(())
}

fn alloc_buffer(caller: &mut Caller<'_, HostState>, size: u32) -> Result<u32, PluginError> {
    let alloc_fn = caller
        .get_export("allocate_buffer")
        .and_then(|e| e.into_func())
        .ok_or_else(|| PluginError::FunctionNotFound("allocate_buffer".into()))?;

    let mut results = [wasmtime::Val::I32(0)];
    alloc_fn
        .call(&mut *caller, &[wasmtime::Val::I32(size as i32)], &mut results)
        .map_err(|e| PluginError::RuntimeError(e.to_string()))?;

    match results[0] {
        wasmtime::Val::I32(ptr) => Ok(ptr as u32),
        _ => Err(PluginError::RuntimeError("invalid return type".into())),
    }
}

pub fn host_log(caller: &mut Caller<'_, HostState>, level: i32, ptr: u32, len: u32) {
    let msg = match read_memory(caller, ptr, len) {
        Ok(data) => String::from_utf8_lossy(&data).to_string(),
        Err(_) => return,
    };

    let plugin_name = caller.data().context.plugin_name.clone();
    match level {
        0 => tracing::trace!("[{}] {}", plugin_name, msg),
        1 => tracing::debug!("[{}] {}", plugin_name, msg),
        2 => tracing::info!("[{}] {}", plugin_name, msg),
        3 => tracing::warn!("[{}] {}", plugin_name, msg),
        4 => tracing::error!("[{}] {}", plugin_name, msg),
        _ => tracing::info!("[{}] {}", plugin_name, msg),
    }
}

pub fn host_storage_get(
    caller: &mut Caller<'_, HostState>,
    key_ptr: u32,
    key_len: u32,
) -> Result<i64, PluginError> {
    let key_bytes = read_memory(caller, key_ptr, key_len)?;
    let key = String::from_utf8_lossy(&key_bytes).to_string();

    let full_key = {
        let state = caller.data();
        format!("{}.{}", state.context.plugin_name, key)
    };

    let found = {
        let state = caller.data();
        state.storage.get(&full_key).map(|r| r.value().clone())
    };

    match found {
        Some(value) => {
            let len = value.len() as u32;
            let buf_ptr = alloc_buffer(caller, len)?;
            write_memory(caller, buf_ptr, &value)?;
            Ok(((buf_ptr as i64) << 32) | (len as i64))
        }
        None => Ok(-1),
    }
}

pub fn host_storage_set(
    caller: &mut Caller<'_, HostState>,
    key_ptr: u32,
    key_len: u32,
    val_ptr: u32,
    val_len: u32,
) -> Result<i32, PluginError> {
    let key_bytes = read_memory(caller, key_ptr, key_len)?;
    let val_bytes = read_memory(caller, val_ptr, val_len)?;
    let key = String::from_utf8_lossy(&key_bytes).to_string();

    let full_key = {
        let state = caller.data();
        format!("{}.{}", state.context.plugin_name, key)
    };

    caller.data().storage.insert(full_key, val_bytes);
    Ok(0)
}

pub fn host_storage_delete(
    caller: &mut Caller<'_, HostState>,
    key_ptr: u32,
    key_len: u32,
) -> Result<i32, PluginError> {
    let key_bytes = read_memory(caller, key_ptr, key_len)?;
    let key = String::from_utf8_lossy(&key_bytes).to_string();

    let full_key = {
        let state = caller.data();
        format!("{}.{}", state.context.plugin_name, key)
    };

    caller.data().storage.remove(&full_key);
    Ok(0)
}

pub fn host_emit_event(
    caller: &mut Caller<'_, HostState>,
    type_ptr: u32,
    type_len: u32,
    data_ptr: u32,
    data_len: u32,
) -> Result<i32, PluginError> {
    let type_bytes = read_memory(caller, type_ptr, type_len)?;
    let data_bytes = read_memory(caller, data_ptr, data_len)?;

    let event_type = String::from_utf8_lossy(&type_bytes).to_string();
    let event_data = String::from_utf8_lossy(&data_bytes).to_string();

    let event = format!("{}:{}", event_type, event_data);

    let event_id = {
        let mut next = caller.data().next_event_id.lock();
        let id = *next;
        *next += 1;
        id
    };

    caller.data().events.entry(event_id).or_default().push(event);
    Ok(0)
}

pub fn host_player_get(
    caller: &mut Caller<'_, HostState>,
    player_id: i64,
) -> Result<i64, PluginError> {
    let json = {
        let state = caller.data();
        match state.players.get(&player_id) {
            Some(player) => {
                rmp_serde::to_vec(&*player).map_err(|e| PluginError::RuntimeError(e.to_string()))?
            }
            None => return Ok(-20),
        }
    };

    let len = json.len() as u32;
    let buf_ptr = alloc_buffer(caller, len)?;
    write_memory(caller, buf_ptr, &json)?;
    Ok(((buf_ptr as i64) << 32) | (len as i64))
}

pub fn host_player_update(
    caller: &mut Caller<'_, HostState>,
    player_id: i64,
    data_ptr: u32,
    data_len: u32,
) -> Result<i32, PluginError> {
    let data_bytes = read_memory(caller, data_ptr, data_len)?;

    let mut player: crate::state::PlayerData =
        rmp_serde::from_slice(&data_bytes).map_err(|e| PluginError::RuntimeError(e.to_string()))?;

    player.id = player_id;

    caller.data().players.insert(player_id, player);
    Ok(0)
}

pub fn host_inventory_get(
    caller: &mut Caller<'_, HostState>,
    player_id: i64,
) -> Result<i64, PluginError> {
    let json = {
        let state = caller.data();
        let items: Vec<crate::state::InventoryEntry> = state
            .inventories
            .get(&player_id)
            .map(|r| r.value().clone())
            .unwrap_or_default();
        rmp_serde::to_vec(&items).map_err(|e| PluginError::RuntimeError(e.to_string()))?
    };

    let len = json.len() as u32;
    let buf_ptr = alloc_buffer(caller, len)?;
    write_memory(caller, buf_ptr, &json)?;
    Ok(((buf_ptr as i64) << 32) | (len as i64))
}

pub fn host_inventory_add_item(
    caller: &mut Caller<'_, HostState>,
    player_id: i64,
    item_id: i64,
    count: i32,
) -> Result<i32, PluginError> {
    let mut items = caller.data().inventories.entry(player_id).or_default();

    if let Some(entry) = items.iter_mut().find(|e| e.item_id == item_id) {
        entry.count += count;
    } else {
        items.push(crate::state::InventoryEntry { item_id, count });
    }

    Ok(0)
}

pub fn host_inventory_remove_item(
    caller: &mut Caller<'_, HostState>,
    player_id: i64,
    item_id: i64,
    count: i32,
) -> Result<i32, PluginError> {
    let mut items = caller.data().inventories.entry(player_id).or_default();

    if let Some(entry) = items.iter_mut().find(|e| e.item_id == item_id) {
        if entry.count >= count {
            entry.count -= count;
            if entry.count == 0 {
                items.retain(|e| e.item_id != item_id);
            }
            Ok(0)
        } else {
            Ok(-1)
        }
    } else {
        Ok(-2)
    }
}

pub fn host_combat_start(
    caller: &mut Caller<'_, HostState>,
    attacker_id: i64,
    defender_id: i64,
) -> Result<i64, PluginError> {
    let combat_id = {
        let mut next = caller.data().next_combat_id.lock();
        let id = *next;
        *next += 1;
        id
    };

    let combat = crate::state::CombatState {
        combat_id,
        attacker_id,
        defender_id,
        turn: 0,
        active: true,
    };

    caller.data().combats.insert(combat_id, combat);
    Ok(combat_id)
}

pub fn host_combat_action(
    caller: &mut Caller<'_, HostState>,
    combat_id: i64,
    action_ptr: u32,
    action_len: u32,
) -> Result<i32, PluginError> {
    let action_bytes = read_memory(caller, action_ptr, action_len)?;
    let action_str = String::from_utf8_lossy(&action_bytes).to_string();

    match caller.data().combats.get_mut(&combat_id) {
        Some(mut combat) => {
            combat.turn += 1;
            tracing::info!("Combat {} turn {}: {}", combat_id, combat.turn, action_str);
            Ok(0)
        }
        None => Ok(-30),
    }
}

pub fn host_send_to_client(
    caller: &mut Caller<'_, HostState>,
    player_id: i64,
    msg_ptr: u32,
    msg_len: u32,
) -> Result<i32, PluginError> {
    let msg_bytes = read_memory(caller, msg_ptr, msg_len)?;
    let msg = String::from_utf8_lossy(&msg_bytes).to_string();

    caller
        .data()
        .messages
        .entry(player_id)
        .or_default()
        .push(msg);
    Ok(0)
}

pub fn host_broadcast_to_room(
    caller: &mut Caller<'_, HostState>,
    room_id: i64,
    msg_ptr: u32,
    msg_len: u32,
) -> Result<i32, PluginError> {
    let msg_bytes = read_memory(caller, msg_ptr, msg_len)?;
    let msg = String::from_utf8_lossy(&msg_bytes).to_string();

    let state = caller.data();

    for entry in state.players.iter() {
        if entry.value().room_id == room_id {
            state
                .messages
                .entry(*entry.key())
                .or_default()
                .push(msg.clone());
        }
    }

    Ok(0)
}
