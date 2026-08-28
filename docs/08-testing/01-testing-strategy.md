# 08-01 - 테스트 전략 상세 설계

## 개요

The Protocol의 테스트 전략은 테스트 피라미드를 기반으로 하며, 유닛 테스트에서 수용 테스트까지 점진적으로 검증 범위를 확대한다.

## 테스트 피라미드

```
              ┌───────────┐
              │  수용 테스트  │  5%
              │(Acceptance)│
             ┌┴───────────┴┐
             │  통합 테스트   │  15%
             │(Integration) │
            ┌┴─────────────┴┐
            │   유닛 테스트    │  80%
            │    (Unit)      │
            └───────────────┘
```

| 레이어 | 비율 | 대상 | 속도 |
|--------|------|------|------|
| Unit | 80% | Domain 로직, 유틸리티 | 빠름 (< 1초) |
| Integration | 15% | DB, 네트워크, 플러그인 | 중간 (1~10초) |
| Acceptance | 5% | 전체 시나리오 E2E | 느림 (10초~분) |

## 유닛 테스트

### Domain 테스트 (순수 로직)

데이터베이스나 네트워크 없이 도메인 로직만 검증한다.

```rust
// domain/src/character.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_character_creation() {
        let character = Character::new("Hero".to_string(), CharacterClass::Warrior);

        assert_eq!(character.name, "Hero");
        assert_eq!(character.class, CharacterClass::Warrior);
        assert_eq!(character.level, 1);
        assert_eq!(character.experience, 0);
        assert!(character.is_alive());
    }

    #[test]
    fn test_warrior_base_stats() {
        let stats = CharacterClass::Warrior.base_stats();
        assert_eq!(stats.strength, 15);
        assert_eq!(stats.constitution, 14);
    }

    #[test]
    fn test_mage_base_stats() {
        let stats = CharacterClass::Mage.base_stats();
        assert_eq!(stats.intelligence, 15);
        assert_eq!(stats.wisdom, 12);
    }

    #[test]
    fn test_take_damage() {
        let mut character = Character::new("Hero".to_string(), CharacterClass::Warrior);
        let initial_hp = character.hp;

        let actual = character.take_damage(20);
        assert_eq!(actual, 20);
        assert_eq!(character.hp, initial_hp - 20);
    }

    #[test]
    fn test_take_damage_lethal() {
        let mut character = Character::new("Hero".to_string(), CharacterClass::Warrior);
        let initial_hp = character.hp;

        let actual = character.take_damage(initial_hp + 100);
        assert_eq!(actual, initial_hp);
        assert_eq!(character.hp, 0);
        assert!(!character.is_alive());
    }

    #[test]
    fn test_heal() {
        let mut character = Character::new("Hero".to_string(), CharacterClass::Warrior);
        character.take_damage(30);
        let damaged_hp = character.hp;

        let healed = character.heal(10);
        assert_eq!(healed, 10);
        assert_eq!(character.hp, damaged_hp + 10);
    }

    #[test]
    fn test_heal_overflow() {
        let mut character = Character::new("Hero".to_string(), CharacterClass::Warrior);
        character.take_damage(10);

        let healed = character.heal(1000);
        assert_eq!(healed, 10);  // 최대 HP를 초과하지 않음
        assert_eq!(character.hp, character.max_hp);
    }

    #[test]
    fn test_gain_experience_no_level_up() {
        let mut character = Character::new("Hero".to_string(), CharacterClass::Warrior);
        let events = character.gain_experience(500);

        assert_eq!(character.experience, 500);
        assert_eq!(character.level, 1);
        assert!(events.is_empty());
    }

    #[test]
    fn test_gain_experience_level_up() {
        let mut character = Character::new("Hero".to_string(), CharacterClass::Warrior);
        let events = character.gain_experience(1000);  // 레벨업 필요 xp: 1 * 1000

        assert_eq!(character.level, 2);
        assert_eq!(character.hp, character.max_hp);  // HP 풀 리커버리
        assert!(!events.is_empty());

        if let DomainEvent::LevelUp { new_level, .. } = &events[0] {
            assert_eq!(*new_level, 2);
        }
    }

    #[test]
    fn test_xp_for_next_level() {
        let mut character = Character::new("Hero".to_string(), CharacterClass::Warrior);
        assert_eq!(character.xp_for_next_level(), 1000);

        character.level = 5;
        assert_eq!(character.xp_for_next_level(), 5000);
    }
}
```

### Inventory 테스트

```rust
// domain/src/inventory.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inventory_creation() {
        let inventory = Inventory::new();
        assert!(inventory.items.is_empty());
        assert_eq!(inventory.capacity, 20);
        assert_eq!(inventory.gold, 0);
    }

    #[test]
    fn test_add_item() {
        let mut inventory = Inventory::new();
        inventory.add_item(1, "Sword", 1).unwrap();

        assert_eq!(inventory.items.len(), 1);
        assert!(inventory.has_item(1, 1));
    }

    #[test]
    fn test_add_item_stack() {
        let mut inventory = Inventory::new();
        inventory.add_item(1, "Potion", 5).unwrap();
        inventory.add_item(1, "Potion", 3).unwrap();

        assert_eq!(inventory.items.len(), 1);
        assert_eq!(inventory.item_count(1), 8);
    }

    #[test]
    fn test_add_item_full() {
        let mut inventory = Inventory::new();
        inventory.capacity = 2;

        for i in 0..5 {
            inventory.add_item(i, &format!("Item {}", i), 1).unwrap();
        }

        assert_eq!(inventory.items.len(), 2);
        assert!(inventory.add_item(99, "Overflow", 1).is_err());
    }

    #[test]
    fn test_remove_item() {
        let mut inventory = Inventory::new();
        inventory.add_item(1, "Sword", 5).unwrap();

        inventory.remove_item(1, 3).unwrap();
        assert_eq!(inventory.item_count(1), 2);
    }

    #[test]
    fn test_remove_item_complete() {
        let mut inventory = Inventory::new();
        inventory.add_item(1, "Sword", 1).unwrap();

        inventory.remove_item(1, 1).unwrap();
        assert_eq!(inventory.items.len(), 0);
        assert!(!inventory.has_item(1, 1));
    }

    #[test]
    fn test_remove_item_insufficient() {
        let mut inventory = Inventory::new();
        inventory.add_item(1, "Sword", 3).unwrap();

        assert!(inventory.remove_item(1, 5).is_err());
        assert_eq!(inventory.item_count(1), 3);
    }

    #[test]
    fn test_remove_item_not_found() {
        let mut inventory = Inventory::new();
        assert!(inventory.remove_item(999, 1).is_err());
    }
}
```

### Combat 테스트

```rust
// domain/src/combat.rs
#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_characters() -> (Character, Character) {
        let mut attacker = Character::new("Attacker".to_string(), CharacterClass::Warrior);
        attacker.id = 1;
        let mut target = Character::new("Target".to_string(), CharacterClass::Mage);
        target.id = 2;
        (attacker, target)
    }

    #[test]
    fn test_damage_calculation() {
        let (attacker, target) = create_test_characters();

        let damage = Combat::calculate_damage(&attacker, &target);

        // 방어력 고려한 데미지 범위 확인
        assert!(damage >= 1);
        assert!(damage < attacker.stats.strength as u32 * 2);
    }

    #[test]
    fn test_process_attack() {
        let (mut attacker, mut target) = create_test_characters();
        let initial_target_hp = target.hp;

        let mut combat = Combat::new(1, 2);
        let events = combat.process_attack(&mut attacker, &mut target);

        assert!(target.hp < initial_target_hp);
        assert!(!combat.log.is_empty());
        assert!(!events.is_empty());
    }

    #[test]
    fn test_combat_end_on_kill() {
        let (mut attacker, mut target) = create_test_characters();
        target.hp = 1;  // 1 HP로 설정

        let mut combat = Combat::new(1, 2);
        let events = combat.process_attack(&mut attacker, &target);

        assert!(!target.is_alive());
        assert!(matches!(combat.state, CombatState::Finished { .. }));
        assert!(events.iter().any(|e| matches!(e, DomainEvent::CombatEnded { .. })));
    }
}
```

### 프로토콜 테스트 (encode/decode)

```rust
// core/protocol/src/codec.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let codec = ProtocolCodec::new();
        let original = Message::ping();

        let encoded = codec.encode(&original).unwrap();
        let mut buf = encoded;
        let decoded = codec.decode_simple(&mut buf).unwrap().unwrap();

        assert_eq!(decoded.version, original.version);
        assert_eq!(decoded.id, original.id);
        assert_eq!(decoded.message_type, original.message_type);
    }

    #[test]
    fn test_encode_decode_command() {
        let codec = ProtocolCodec::new();
        let move_cmd = MoveCommand {
            direction: Direction::North,
        };
        let payload = rmp_serde::to_vec(&move_cmd).unwrap();
        let original = Message::command(Command {
            id: 42,
            command_type: "move".to_string(),
            session_id: 1,
            timestamp: 1234567890,
            payload,
        });

        let encoded = codec.encode(&original).unwrap();
        let mut buf = encoded;
        let decoded = codec.decode_simple(&mut buf).unwrap().unwrap();

        assert_eq!(decoded.message_type, MessageType::Command);
        let cmd: Command = rmp_serde::from_slice(&decoded.payload).unwrap();
        assert_eq!(cmd.command_type, "move");
        assert_eq!(cmd.id, 42);
    }

    #[test]
    fn test_message_type_from_u8() {
        assert_eq!(MessageType::from_u8(0x01), Some(MessageType::Command));
        assert_eq!(MessageType::from_u8(0x02), Some(MessageType::CommandResponse));
        assert_eq!(MessageType::from_u8(0x10), Some(MessageType::Event));
        assert_eq!(MessageType::from_u8(0x20), Some(MessageType::Ping));
        assert_eq!(MessageType::from_u8(0xFF), None);
    }

    #[test]
    fn test_decode_incomplete_frame() {
        let codec = ProtocolCodec::new();
        let mut buf = bytes::BytesMut::from(&[0u8; 10][..]);

        let result = codec.decode_simple(&mut buf).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_hello_message() {
        let codec = ProtocolCodec::new();
        let original = Message::hello(ClientType::MUD, Some("token123".to_string()));

        let encoded = codec.encode(&original).unwrap();
        let mut buf = encoded;
        let decoded = codec.decode_simple(&mut buf).unwrap().unwrap();

        assert_eq!(decoded.message_type, MessageType::Hello);
        let hello: Hello = rmp_serde::from_slice(&decoded.payload).unwrap();
        assert_eq!(hello.client_type, ClientType::MUD);
        assert_eq!(hello.auth_token, Some("token123".to_string()));
    }

    #[test]
    fn test_direction_from_str() {
        assert_eq!(Direction::from_str("north"), Some(Direction::North));
        assert_eq!(Direction::from_str("n"), Some(Direction::North));
        assert_eq!(Direction::from_str("s"), Some(Direction::South));
        assert_eq!(Direction::from_str("up"), Some(Direction::Up));
        assert_eq!(Direction::from_str("invalid"), None);
    }
}
```

## 통합 테스트

### DB 테스트 (sqlx::test)

```rust
// tests/integration/db_test.rs
use sqlx::PgPool;

#[sqlx::test(migrations = "./migrations")]
async fn test_create_character(pool: PgPool) -> sqlx::Result<()> {
    let repo = PostgresCharacterRepository::new(pool.clone());

    // 계정 생성
    let account = sqlx::query!(
        "INSERT INTO accounts (username, email, password_hash) VALUES ($1, $2, $3) RETURNING id",
        "testuser",
        "test@example.com",
        "hashed_password"
    )
    .fetch_one(&pool)
    .await?;

    // 캐릭터 생성
    let new_char = NewCharacter {
        account_id: account.id,
        name: "Hero".to_string(),
        class: CharacterClass::Warrior,
        hp: 80,
        max_hp: 80,
        mp: 28,
        max_mp: 28,
        stats: CharacterClass::Warrior.base_stats(),
        room_id: 1,
    };

    let character = repo.create(&new_char).await?;
    assert_eq!(character.name, "Hero");
    assert_eq!(character.class, CharacterClass::Warrior);

    // 조회
    let found = repo.find_by_id(character.id).await?;
    assert!(found.is_some());

    // 이름으로 조회
    let by_name = repo.find_by_name("Hero").await?;
    assert!(by_name.is_some());

    // 계정으로 조회
    let by_account = repo.find_by_account(account.id).await?;
    assert_eq!(by_account.len(), 1);

    Ok(())
}

#[sqlx::test(migrations = "./migrations")]
async fn test_inventory_operations(pool: PgPool) -> sqlx::Result<()> {
    let repo = PostgresInventoryRepository::new(pool.clone());

    // 테스트 캐릭터 생성
    let character_id = create_test_character(&pool).await?;

    // 아이템 추가
    let new_item = NewItem {
        item_id: 100,
        item_name: "Iron Sword".to_string(),
        quantity: 1,
        item_type: "Weapon".to_string(),
    };
    let item = repo.add_item(character_id, &new_item).await?;
    assert_eq!(item.quantity, 1);

    // 수량 업데이트
    repo.update_quantity(item.id, 5).await?;
    let items = repo.find_by_character(character_id).await?;
    assert_eq!(items[0].quantity, 5);

    // 아이템 제거
    repo.remove_item(character_id, 100, 3).await?;
    let items = repo.find_by_character(character_id).await?;
    assert_eq!(items[0].quantity, 2);

    Ok(())
}
```

### 네트워크 테스트 (TCP 연결)

```rust
// tests/integration/network_test.rs
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn test_tcp_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut len_buf = [0u8; 4];
        socket.read_exact(&mut len_buf).await.unwrap();
        let total_len = u32::from_be_bytes(len_buf) as usize;
        let mut frame = vec![0u8; total_len - 4];
        socket.read_exact(&mut frame).await.unwrap();

        // HelloAck 전송
        let codec = ProtocolCodec::new();
        let ack = Message::hello_ack(1, vec!["game".to_string()]);
        let ack_bytes = codec.encode(&ack).unwrap();
        socket.write_all(&ack_bytes).await.unwrap();
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    client.set_nodelay(true).unwrap();

    // Hello 전송
    let codec = ProtocolCodec::new();
    let hello = Message::hello(ClientType::MUD, None);
    let hello_bytes = codec.encode(&hello).unwrap();
    client.write_all(&hello_bytes).await.unwrap();

    // HelloAck 수신
    let mut len_buf = [0u8; 4];
    client.read_exact(&mut len_buf).await.unwrap();
    let total_len = u32::from_be_bytes(len_buf) as usize;
    let mut frame = vec![0u8; total_len - 4];
    client.read_exact(&mut frame).await.unwrap();

    let mut full_frame = bytes::BytesMut::with_capacity(4 + total_len);
    full_frame.put_slice(&len_buf);
    full_frame.put_slice(&frame);

    let mut buf = full_frame;
    let ack = ProtocolCodec::decode_simple(&mut buf).unwrap().unwrap();
    assert_eq!(ack.message_type, MessageType::HelloAck);

    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_session_manager() {
    let manager = SessionManager::new(10);
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

    let session_id = manager.create_session(addr, TransportType::Tcp).unwrap();
    assert!(session_id > 0);
    assert_eq!(manager.count(), 1);

    let session = manager.get(session_id).unwrap();
    assert_eq!(session.address, addr);
    assert_eq!(session.state, SessionState::Connected);

    manager.remove(session_id);
    assert_eq!(manager.count(), 0);
}
```

## 수용 테스트 (Acceptance Test)

### Terminal A / Terminal B 시나리오

```
Terminal A (서버):
  1. runtime server --bind 127.0.0.1:7770
  2. 서버 시작 대기
  3. 클라이언트 연결 대기

Terminal B (클라이언트):
  1. runtime client --server 127.0.0.1:7770
  2. 핸드셰이크 완료
  3. create Hero warrior
  4. look
  5. move north
  6. attack goblin
  7. inventory
  8. quit
```

### 자동화된 수용 테스트

```rust
// tests/acceptance/full_game_flow.rs
#[tokio::test]
async fn test_full_game_flow() {
    // 서버 시작
    let server = start_test_server("127.0.0.1:0").await;
    let addr = server.local_addr();

    // 클라이언트 연결
    let mut client = connect_client(&addr).await;

    // 1. 핸드셰이크
    let session_id = client.handshake().await;
    assert!(session_id > 0);

    // 2. 캐릭터 생성
    let response = client.send_command("create", &CreateCharacterCommand {
        name: "Hero".to_string(),
        class: "warrior".to_string(),
    }).await;
    assert!(response.success);
    let character_id = response.character_id.unwrap();

    // 3. 주변 둘러보기
    let response = client.send_command("look", &()).await;
    assert!(response.success);
    let look: LookResponse = response.parse().unwrap();
    assert_eq!(look.room_name, "Town Square");

    // 4. 이동
    let response = client.send_command("move", &MoveCommand {
        direction: Direction::North,
    }).await;
    assert!(response.success);

    // 5. 전투
    let response = client.send_command("attack", &AttackCommand {
        target_id: 4,  // Goblin
    }).await;
    assert!(response.success);

    // 6. 인벤토리 확인
    let response = client.send_command("inventory", &()).await;
    assert!(response.success);

    // 7. 종료
    client.disconnect().await;

    server.shutdown().await;
}
```

### HTTP API 테스트

```rust
// tests/acceptance/http_api.rs
#[tokio::test]
async fn test_http_api_endpoints() {
    let app = create_test_app().await;
    let client = reqwest::Client::new();
    let base_url = format!("http://{}", app.local_addr());

    // 1. 로그인
    let response = client.post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({
            "username": "testuser",
            "password": "password123"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    let token = body["access_token"].as_str().unwrap();

    // 2. 캐릭터 조회
    let response = client.get(format!("{}/api/v1/characters/1", base_url))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // 3. 랭킹 조회
    let response = client.get(format!("{}/api/v1/ranking", base_url))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}
```

## 테스트 코드 예시

### 테스트 유틸리티

```rust
// tests/common/mod.rs
use std::net::SocketAddr;
use tokio::net::TcpListener;

pub async fn start_test_server(addr: &str) -> TestServer {
    let listener = TcpListener::bind(addr).await.unwrap();
    let local_addr = listener.local_addr().unwrap();

    let session_manager = Arc::new(SessionManager::new(100));
    let game_world = Arc::new(RwLock::new(GameWorld::new()));

    let handle = tokio::spawn(async move {
        // 서버 루프
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let sm = session_manager.clone();
            let gw = game_world.clone();

            tokio::spawn(async move {
                handle_connection(socket, sm, gw).await;
            });
        }
    });

    TestServer {
        addr: local_addr,
        handle,
    }
}

pub struct TestServer {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    pub async fn shutdown(self) {
        self.handle.abort();
    }
}

pub struct TestClient {
    stream: tokio::net::TcpStream,
    codec: ProtocolCodec,
}

impl TestClient {
    pub async fn connect(addr: &SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).await.unwrap();
        Self {
            stream,
            codec: ProtocolCodec::new(),
        }
    }

    pub async fn handshake(&mut self) -> u64 {
        let hello = Message::hello(ClientType::MUD, None);
        let bytes = self.codec.encode(&hello).unwrap();
        self.stream.write_all(&bytes).await.unwrap();

        // HelloAck 수신
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await.unwrap();
        let total_len = u32::from_be_bytes(len_buf) as usize;
        let mut frame = vec![0u8; total_len - 4];
        self.stream.read_exact(&mut frame).await.unwrap();

        let mut full_frame = BytesMut::with_capacity(4 + total_len);
        full_frame.put_slice(&len_buf);
        full_frame.put_slice(&frame);

        let mut buf = full_frame;
        let ack = ProtocolCodec::decode_simple(&mut buf).unwrap().unwrap();
        let hello_ack: HelloAck = rmp_serde::from_slice(&ack.payload).unwrap();
        hello_ack.session_id
    }
}
```

## 커버리지 목표

| 모듈 | 커버리지 목표 | 현재 추정 |
|------|-------------|----------|
| domain/ | 90% | 85% |
| application/ | 80% | 70% |
| core/protocol/ | 85% | 80% |
| core/session/ | 80% | 75% |
| core/routing/ | 80% | 70% |
| core/security/ | 75% | 60% |
| core/network/ | 60% | 50% |
| 전체 | 80% | 70% |

```bash
# 커버리지 리포트 생성
cargo tarpaulin --workspace --out Html --output-dir coverage
cargo tarpaulin --workspace --out Xml --output-dir coverage
```
