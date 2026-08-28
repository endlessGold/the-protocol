# 09-01 - 캐릭터 플러그인 상세 설계

## 개요

캐릭터 플러그인은 The Protocol의 가장 기본적인 플러그인으로, 캐릭터 생성/조회/삭제 및 레벨업 이벤트를 처리한다. WASM 샌드박스 환경에서 실행되며, Host Function을 통해 런타임 리소스에 접근한다.

## 캐릭터 플러그인 아키텍처

```
┌──────────────────────────────────────────────────────────┐
│                    WASM 샌드박스                          │
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │              Character Plugin                      │ │
│  │                                                    │ │
│  │  ┌──────────────┐  ┌────────────────────────────┐ │ │
│  │  │   Command    │  │      Event Handlers        │ │ │
│  │  │   Handlers   │  │                            │ │ │
│  │  │              │  │  - character_created        │ │ │
│  │  │  - login     │  │  - level_up                 │ │ │
│  │  │  - create    │  │                            │ │ │
│  │  │  - delete    │  └────────────────────────────┘ │ │
│  │  │  - get       │                                  │ │
│  │  └──────────────┘                                  │ │
│  └────────────────────────┬───────────────────────────┘ │
│                           │ Host Function Calls          │
│  ┌────────────────────────▼───────────────────────────┐ │
│  │                 Host Functions                     │ │
│  │                                                    │ │
│  │  - player_get(id)     → PlayerData                 │ │
│  │  - player_update(id, data) → Result                │ │
│  │  - storage_get(key)   → Vec<u8>                    │ │
│  │  - storage_set(key, value) → Result                │ │
│  └────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

## 커맨드 핸들러

### login

플레이어 인증 및 세션 생성을 처리한다.

```rust
// 플러그인 측 핸들러 (WASM 내부)
#[no_mangle]
pub extern "C" fn handle_login(input_ptr: *const u8, input_len: usize) -> *mut u8 {
    let input = unsafe {
        std::slice::from_raw_parts(input_ptr, input_len)
    };

    let cmd: LoginCommand = rmp_serde::from_slice(input).unwrap();

    // Host Function으로 플레이어 데이터 조회
    let player_data = host_player_get_by_username(&cmd.username);

    match player_data {
        Some(player) => {
            // 패스워드 검증 (Host Function에서 수행)
            let verified = host_verify_password(&cmd.password, &player.password_hash);

            if verified {
                let response = LoginResponse {
                    success: true,
                    session_id: host_session_id(),
                    player_id: Some(player.id),
                    error: None,
                };
                let bytes = rmp_serde::to_vec(&response).unwrap();
                return_to_host(&bytes)
            } else {
                let response = LoginResponse {
                    success: false,
                    session_id: 0,
                    player_id: None,
                    error: Some("Invalid password".to_string()),
                };
                let bytes = rmp_serde::to_vec(&response).unwrap();
                return_to_host(&bytes)
            }
        }
        None => {
            let response = LoginResponse {
                success: false,
                session_id: 0,
                player_id: None,
                error: Some("Player not found".to_string()),
            };
            let bytes = rmp_serde::to_vec(&response).unwrap();
            return_to_host(&bytes)
        }
    }
}
```

### create_character

새 캐릭터를 생성한다.

```rust
#[no_mangle]
pub extern "C" fn handle_create_character(input_ptr: *const u8, input_len: usize) -> *mut u8 {
    let input = unsafe {
        std::slice::from_raw_parts(input_ptr, input_len)
    };

    let cmd: CreateCharacterCommand = rmp_serde::from_slice(input).unwrap();

    // 이름 유효성 검증
    if cmd.name.len() < 2 || cmd.name.len() > 32 {
        let response = CreateCharacterResponse {
            success: false,
            character_id: None,
            error: Some("Name must be 2-32 characters".to_string()),
        };
        let bytes = rmp_serde::to_vec(&response).unwrap();
        return_to_host(&bytes);
    }

    // 클래스 검증
    let class = match cmd.class.to_lowercase().as_str() {
        "warrior" => CharacterClass::Warrior,
        "mage" => CharacterClass::Mage,
        "rogue" => CharacterClass::Rogue,
        "cleric" => CharacterClass::Cleric,
        _ => {
            let response = CreateCharacterResponse {
                success: false,
                character_id: None,
                error: Some(format!("Invalid class: {}", cmd.class)),
            };
            let bytes = rmp_serde::to_vec(&response).unwrap();
            return_to_host(&bytes);
        }
    };

    // 이름 중복 검사 (Host Function)
    if host_character_name_exists(&cmd.name) {
        let response = CreateCharacterResponse {
            success: false,
            character_id: None,
            error: Some("Character name already taken".to_string()),
        };
        let bytes = rmp_serde::to_vec(&response).unwrap();
        return_to_host(&bytes);
    }

    // 캐릭터 생성 (도메인 로직)
    let base_stats = class.base_stats();
    let max_hp = 50 + (base_stats.constitution * 2);

    let character = Character {
        id: 0,  // Host가 할당
        name: cmd.name,
        class,
        level: 1,
        experience: 0,
        hp: max_hp,
        max_hp,
        mp: 20 + base_stats.wisdom,
        max_mp: 20 + base_stats.wisdom,
        stats: base_stats,
        room_id: 1,  // 시작 방
        inventory: Inventory::new(),
    };

    // Host Function으로 저장
    let character_id = host_character_create(&character);

    // 이벤트 발행 (Host Function)
    host_emit_event(DomainEvent::CharacterCreated {
        character_id,
        name: character.name.clone(),
    });

    let response = CreateCharacterResponse {
        success: true,
        character_id: Some(character_id),
        error: None,
    };
    let bytes = rmp_serde::to_vec(&response).unwrap();
    return_to_host(&bytes)
}
```

### delete_character

캐릭터를软删除(soft delete)한다.

```rust
#[no_mangle]
pub extern "C" fn handle_delete_character(input_ptr: *const u8, input_len: usize) -> *mut u8 {
    let input = unsafe {
        std::slice::from_raw_parts(input_ptr, input_len)
    };

    let cmd: DeleteCharacterCommand = rmp_serde::from_slice(input).unwrap();

    // 소유권 검증
    let player_id = host_current_player_id();
    let character = host_character_get(cmd.character_id);

    match character {
        Some(ch) if ch.account_id == player_id => {
            // 소유자 확인됨
            host_character_delete(cmd.character_id);

            let response = DeleteCharacterResponse {
                success: true,
                error: None,
            };
            let bytes = rmp_serde::to_vec(&response).unwrap();
            return_to_host(&bytes)
        }
        Some(_) => {
            let response = DeleteCharacterResponse {
                success: false,
                error: Some("Not your character".to_string()),
            };
            let bytes = rmp_serde::to_vec(&response).unwrap();
            return_to_host(&bytes)
        }
        None => {
            let response = DeleteCharacterResponse {
                success: false,
                error: Some("Character not found".to_string()),
            };
            let bytes = rmp_serde::to_vec(&response).unwrap();
            return_to_host(&bytes)
        }
    }
}
```

### get_character

캐릭터 정보를 조회한다.

```rust
#[no_mangle]
pub extern "C" fn handle_get_character(input_ptr: *const u8, input_len: usize) -> *mut u8 {
    let input = unsafe {
        std::slice::from_raw_parts(input_ptr, input_len)
    };

    let cmd: GetCharacterCommand = rmp_serde::from_slice(input).unwrap();

    let character = host_character_get(cmd.character_id);

    match character {
        Some(ch) => {
            let response = GetCharacterResponse {
                success: true,
                character: Some(ch),
                error: None,
            };
            let bytes = rmp_serde::to_vec(&response).unwrap();
            return_to_host(&bytes)
        }
        None => {
            let response = GetCharacterResponse {
                success: false,
                character: None,
                error: Some("Character not found".to_string()),
            };
            let bytes = rmp_serde::to_vec(&response).unwrap();
            return_to_host(&bytes)
        }
    }
}
```

## 이벤트 핸들러

### character_created

캐릭터 생성 후 초기 설정을 수행한다.

```rust
#[no_mangle]
pub extern "C" fn handle_character_created(input_ptr: *const u8, input_len: usize) -> *mut u8 {
    let input = unsafe {
        std::slice::from_raw_parts(input_ptr, input_len)
    };

    let event: DomainEvent = rmp_serde::from_slice(input).unwrap();

    if let DomainEvent::CharacterCreated { character_id, name } = event {
        // 초기 인벤토리 아이템 지급
        host_inventory_add(character_id, 1, "Rusty Sword", 1);
        host_inventory_add(character_id, 2, "Cloth Armor", 1);
        host_inventory_add(character_id, 10, "Health Potion", 5);

        // 환영 메시지 발행
        host_emit_event(DomainEvent::PlayerEnteredRoom {
            player_id: character_id,
            room_id: 1,
        });

        tracing::info!(
            character_id,
            name = %name,
            "Character created event processed"
        );
    }

    return_to_host(&[]);
}
```

### level_up

레벨업 시 스탯 증가 및 보상 지급을 처리한다.

```rust
#[no_mangle]
pub extern "C" fn handle_level_up(input_ptr: *const u8, input_len: usize) -> *mut u8 {
    let input = unsafe {
        std::slice::from_raw_parts(input_ptr, input_len)
    };

    let event: DomainEvent = rmp_serde::from_slice(input).unwrap();

    if let DomainEvent::LevelUp { character_id, new_level } = event {
        let mut character = host_character_get(character_id).unwrap();

        // 레벨업 보상: 골드 지급
        let gold_reward = 100 * new_level as u64;
        host_character_add_gold(character_id, gold_reward);

        // 10레벨마다 추가 보상
        if new_level % 10 == 0 {
            host_inventory_add(character_id, 99, "Rare Chest", 1);
            tracing::info!(
                character_id,
                new_level,
                "Special reward at level milestone"
            );
        }

        // 스탯 포인트 분배 (선택적)
        // 각 레벨업 시 스탯 1포인트 추가
        character.stats.strength += 1;
        character.stats.dexterity += 1;
        character.stats.intelligence += 1;
        character.stats.wisdom += 1;
        character.stats.constitution += 1;

        host_character_update(&character);

        tracing::info!(
            character_id,
            new_level,
            gold_reward,
            "Level up processed"
        );
    }

    return_to_host(&[]);
}
```

## Host Function 사용

### player_get

플레이어 기본 정보를 조회한다.

```rust
// 플러그인 측 선언
extern "C" {
    fn host_player_get(player_id: u64) -> *const u8;
    fn host_player_get_length(player_id: u64) -> usize;
}

// 편의 래퍼
fn host_player_get_wrapper(player_id: u64) -> Option<PlayerData> {
    let len = unsafe { host_player_get_length(player_id) };
    if len == 0 {
        return None;
    }

    let ptr = unsafe { host_player_get(player_id) };
    let data = unsafe {
        std::slice::from_raw_parts(ptr, len)
    };

    rmp_serde::from_slice(data).ok()
}
```

### player_update

플레이어 정보를 업데이트한다.

```rust
extern "C" {
    fn host_player_update(player_id: u64, data_ptr: *const u8, data_len: usize) -> i32;
}

fn host_player_update_wrapper(player_id: u64, player: &PlayerData) -> Result<(), String> {
    let data = rmp_serde::to_vec(player).map_err(|e| e.to_string())?;
    let result = unsafe {
        host_player_update(player_id, data.as_ptr(), data.len())
    };

    if result == 0 {
        Ok(())
    } else {
        Err("Failed to update player".to_string())
    }
}
```

### storage_get / storage_set

플러그인 전용 키-값 저장소에 접근한다.

```rust
extern "C" {
    fn host_storage_get(key_ptr: *const u8, key_len: usize) -> *const u8;
    fn host_storage_get_length(key_ptr: *const u8, key_len: usize) -> usize;
    fn host_storage_set(key_ptr: *const u8, key_len: usize,
                        value_ptr: *const u8, value_len: usize) -> i32;
}

fn storage_get(key: &str) -> Option<Vec<u8>> {
    let key_bytes = key.as_bytes();
    let len = unsafe {
        host_storage_get_length(key_bytes.as_ptr(), key_bytes.len())
    };
    if len == 0 {
        return None;
    }

    let ptr = unsafe {
        host_storage_get(key_bytes.as_ptr(), key_bytes.len())
    };
    let data = unsafe {
        std::slice::from_raw_parts(ptr, len)
    };
    Some(data.to_vec())
}

fn storage_set(key: &str, value: &[u8]) -> Result<(), String> {
    let key_bytes = key.as_bytes();
    let result = unsafe {
        host_storage_set(
            key_bytes.as_ptr(), key_bytes.len(),
            value.as_ptr(), value.len(),
        )
    };

    if result == 0 {
        Ok(())
    } else {
        Err("Failed to set storage".to_string())
    }
}
```

## 매니페스트 (plugin.toml)

```toml
[package]
name = "character"
version = "1.0.0"
description = "Character management plugin for The Protocol"
author = "Aetherius Team"
license = "MIT"

[api]
version = "1.0.0"
minimum_runtime_version = "1.0.0"

[permissions]
required = ["player.read", "player.modify"]
optional = ["inventory.read", "combat.read"]

[resources]
memory_limit = "64MB"
execution_limit = "100ms"
storage_access = true

[dependencies]
# 이 플러그인은 최초 로딩되므로 의존성 없음

[commands]
register = ["login", "create_character", "delete_character", "get_character"]

[events]
subscribe = ["character_created", "level_up"]

[metadata]
category = "core"
priority = 100  # 최우선 로딩
```

## 권한

| 권한 | 필요 | 설명 |
|------|------|------|
| player.read | 필수 | 플레이어 정보 조회 |
| player.modify | 필수 | 플레이어 정보 수정 |
| inventory.read | 선택 | 인벤토리 조회 (초기 아이템 지급) |
| combat.read | 선택 | 전투 정보 조회 |

## 의존성: 없음 (최초 로딩)

캐릭터 플러그인은 다른 플러그인에 의존하지 않으므로 최초에 로딩된다. 다른 플러그인들이 이 플러그인의 데이터를 참조할 수 있다.

```
로딩 순서:
1. character (최초)  ← 여기
2. inventory
3. combat
4. auction
5. guild
...
```

## WASM 빌드

```bash
# WASM 타겟 빌드
cargo build --release --target wasm32-wasip1 -p character-plugin

# 출력 파일
target/wasm32-wasip1/release/character.wasm
```
