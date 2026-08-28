# 18 - Testing

## Overview

Testing strategy covers unit tests, integration tests, and acceptance tests. The architecture's clean layer separation makes each layer independently testable.

## Testing Pyramid

```
                    ┌─────────┐
                    │Acceptance│  ← Full system tests
                    │  Tests   │    (server + client)
                    └────┬────┘
                         │
                 ┌───────▼───────┐
                 │  Integration   │  ← Cross-layer tests
                 │    Tests       │    (DB, network, plugins)
                 └───────┬───────┘
                         │
            ┌────────────▼────────────┐
            │       Unit Tests        │  ← Pure logic tests
            │  (Domain, Application)  │    (fast, isolated)
            └─────────────────────────┘
```

## Unit Tests

### Domain Tests (No Dependencies)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_character_creation() {
        let char = Character::new("Hero".into(), CharacterClass::Warrior);
        assert_eq!(char.name, "Hero");
        assert_eq!(char.level, 1);
        assert!(char.is_alive());
    }

    #[test]
    fn test_damage_and_heal() {
        let mut char = Character::new("Hero".into(), CharacterClass::Warrior);
        let max_hp = char.max_hp;

        char.take_damage(10);
        assert_eq!(char.hp, max_hp - 10);

        char.heal(5);
        assert_eq!(char.hp, max_hp - 5);

        char.heal(100); // Overheal should cap at max
        assert_eq!(char.hp, max_hp);
    }

    #[test]
    fn test_experience_and_leveling() {
        let mut char = Character::new("Hero".into(), CharacterClass::Warrior);
        let events = char.gain_experience(1000);
        assert_eq!(char.level, 2);
        assert!(!events.is_empty());
    }

    #[test]
    fn test_combat_damage_calculation() {
        let attacker = Character::new("Attacker".into(), CharacterClass::Warrior);
        let target = Character::new("Target".into(), CharacterClass::Mage);

        let damage = Combat::calculate_damage(&attacker, &target, None);
        assert!(damage > 0);
        assert!(damage < 100); // Reasonable range
    }

    #[test]
    fn test_inventory_add_remove() {
        let mut inv = Inventory::new();
        inv.add_item(1, 5).unwrap();
        assert!(inv.has_item(1, 5));

        inv.remove_item(1, 3).unwrap();
        assert!(inv.has_item(1, 2));
        assert!(!inv.has_item(1, 3));
    }

    #[test]
    fn test_inventory_capacity() {
        let mut inv = Inventory::new();
        inv.capacity = 2;

        inv.add_item(1, 1).unwrap();
        inv.add_item(2, 1).unwrap();
        assert!(inv.add_item(3, 1).is_err()); // Should fail
    }
}
```

### Application Service Tests (Mocked Repositories)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mockall::mock;
    use mockall::predicate::*;

    mock! {
        pub CharacterRepo {}
        #[async_trait]
        impl CharacterRepository for CharacterRepo {
            async fn find_by_id(&self, id: u64) -> Result<Option<Character>>;
            async fn find_by_name(&self, name: &str) -> Result<Option<Character>>;
            async fn find_by_account(&self, account_id: u64) -> Result<Vec<Character>>;
            async fn save(&self, character: &Character) -> Result<Character>;
            async fn delete(&self, id: u64) -> Result<()>;
        }
    }

    #[tokio::test]
    async fn test_create_character() {
        let mut mock_repo = MockCharacterRepo::new();
        mock_repo.expect_save()
            .returning(|c| Ok(c.clone()));

        let service = PostgresCharacterService {
            repo: Arc::new(mock_repo),
        };

        let result = service.create_character(
            1,
            "Hero".into(),
            CharacterClass::Warrior,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "Hero");
    }

    #[tokio::test]
    async fn test_create_character_invalid_name() {
        let mock_repo = MockCharacterRepo::new();
        let service = PostgresCharacterService {
            repo: Arc::new(mock_repo),
        };

        let result = service.create_character(
            1,
            "ab".into(), // Too short
            CharacterClass::Warrior,
        ).await;

        assert!(result.is_err());
    }
}
```

## Integration Tests

### Database Tests

```rust
#[cfg(test)]
mod integration {
    use sqlx::PgPool;

    #[sqlx::test]
    async fn test_character_crud(pool: PgPool) {
        let repo = PostgresCharacterRepository { pool };

        let character = Character::new("TestHero".into(), CharacterClass::Warrior);
        let saved = repo.save(&character).await.unwrap();

        let found = repo.find_by_id(saved.id).await.unwrap().unwrap();
        assert_eq!(found.name, "TestHero");

        repo.delete(found.id).await.unwrap();
        assert!(repo.find_by_id(found.id).await.unwrap().is_none());
    }
}
```

### Network Tests

```rust
#[tokio::test]
async fn test_tcp_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap;
        // Handle connection
    });

    let client = TcpStream::connect(addr).await.unwrap;
    // Send test message

    server.await.unwrap();
}
```

### Plugin Tests

```rust
#[tokio::test]
async fn test_plugin_loading() {
    let config = PluginConfig {
        directory: PathBuf::from("test_plugins"),
        ..Default::default()
    };

    let mut manager = PluginManager::new(config).await.unwrap();

    let manifests = manager.discover().await.unwrap();
    assert!(!manifests.is_empty());

    for manifest in &manifests {
        manager.validate(manifest).await.unwrap();
        manager.load(manifest).await.unwrap();
        manager.initialize(&manifest.name).await.unwrap();
        manager.enable(&manifest.name).await.unwrap();
    }

    // Verify plugins are enabled
    assert_eq!(manager.get_enabled_plugins().len(), manifests.len());
}
```

## Acceptance Tests

### MUD Client Test

```rust
#[tokio::test]
async fn test_full_mud_session() {
    // Start server
    let server_config = RuntimeConfig::from_file("test_server.toml").unwrap();
    let mut server = ServerRuntime::new(server_config).await.unwrap();
    tokio::spawn(async move { server.start().await.unwrap(); });

    // Wait for server to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect client
    let client_config = ClientConfig {
        server_address: "127.0.0.1:7770".into(),
        ..Default::default()
    };
    let mut client = ClientRuntime::new(client_config).await.unwrap();
    client.connect().await.unwrap();

    // Login
    let login_result = client.login("testuser", "password").await.unwrap();
    assert!(login_result.success);

    // Create character
    let char_result = client.create_character("TestHero", "Warrior").await.unwrap();
    assert!(char_result.success);

    // Look around
    let room = client.look().await.unwrap();
    assert!(!room.description.is_empty());

    // Move
    let move_result = client.move_dir(Direction::North).await.unwrap();
    assert!(move_result.success);

    // Attack
    let attack_result = client.attack(1).await.unwrap(); // Attack NPC #1
    assert!(attack_result.success);

    // Check inventory
    let inventory = client.inventory().await.unwrap();
    assert!(inventory.items.len() >= 0);

    // Disconnect
    client.disconnect().await.unwrap();
}
```

### HTTP API Test

```rust
#[tokio::test]
async fn test_http_api() {
    let app = create_test_app().await;
    let server = axum::test::init_service(app).await;

    // Health check
    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let resp = server.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Get character
    let req = Request::builder()
        .uri("/api/v1/characters/1")
        .body(Body::empty())
        .unwrap();
    let resp = server.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
```

## Test Configuration

```toml
# test_server.toml
[runtime]
mode = "server"
name = "test-server"

[server]
bind_address = "127.0.0.1:17770"  # Different port for tests
max_connections = 100

[database.postgres]
url = "postgresql://localhost:5432/the_protocol_test"

[plugins]
directory = "test_plugins"
```

## Test Categories

| Category | Speed | Isolation | Dependencies |
|----------|-------|-----------|-------------|
| Domain unit tests | Fast (<1ms) | None | None |
| Application unit tests | Fast (<10ms) | Mocked repos | None |
| Integration tests | Medium (<1s) | Separate DB | PostgreSQL, Redis |
| Network tests | Medium (<1s) | Localhost | None |
| Plugin tests | Medium (<5s) | WASM sandbox | None |
| Acceptance tests | Slow (>5s) | Full stack | All services |

## Running Tests

```bash
# Unit tests only (fast)
cargo test --lib

# Integration tests
cargo test --test integration

# All tests
cargo test

# With coverage
cargo tarpaulin --out Html

# Specific test
cargo test test_character_creation
```

## CI Test Matrix

```yaml
test:
  strategy:
    matrix:
      os: [ubuntu-latest, windows-latest]
      rust: [stable, beta]

  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@master
      with:
        toolchain: ${{ matrix.rust }}

    - name: Run tests
      run: cargo test --all

    - name: Run clippy
      run: cargo clippy --all -- -D warnings

    - name: Check formatting
      run: cargo fmt --all -- --check
```

## References

- [01-architecture.md](01-architecture.md) - Architecture overview
- [12-domain.md](12-domain.md) - Domain entities (unit tested)
- [13-application.md](13-application.md) - Application services (integration tested)
