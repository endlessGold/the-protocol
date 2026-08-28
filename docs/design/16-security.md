# 16 - Security

## Overview

Security is integrated at multiple layers: authentication, authorization, capability enforcement, network security, and plugin sandboxing.

## Security Layers

```
┌─────────────────────────────────────────┐
│  Network Security                        │
│  TLS, Rate Limiting, DDoS Protection    │
├─────────────────────────────────────────┤
│  Authentication                          │
│  JWT, Password Hashing, Session Mgmt    │
├─────────────────────────────────────────┤
│  Authorization                           │
│  Role-based, Permission-based           │
├─────────────────────────────────────────┤
│  Capability Enforcement                  │
│  Plugin permissions, Resource limits    │
├─────────────────────────────────────────┤
│  WASM Sandbox                            │
│  Memory limits, Execution limits, WASI  │
├─────────────────────────────────────────┤
│  Input Validation                        │
│  Command validation, Message limits     │
└─────────────────────────────────────────┘
```

## Authentication

### JWT Tokens

```rust
pub struct AuthManager {
    secret: String,
    token_expiry: Duration,
    refresh_expiry: Duration,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: u64,        // Player ID
    pub exp: usize,      // Expiry
    pub iat: usize,      // Issued at
    pub permissions: Vec<String>,
    pub session_id: u64,
}

impl AuthManager {
    pub fn authenticate(&self, token: &str) -> Result<Claims> {
        let validation = Validation::default();
        let token_data = jsonwebtoken::decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        )?;
        Ok(token_data.claims)
    }

    pub fn create_token(&self, claims: &Claims) -> Result<String> {
        jsonwebtoken::encode(
            &Header::default(),
            claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
    }
}
```

### Password Hashing

```rust
pub fn hash_password(password: &str) -> Result<String> {
    argon2::hash_encoded(
        password.as_bytes(),
        &rand::thread_rng().gen::<[u8; 32]>(),
        &Argon2::default(),
    )
}

pub fn verify_password(hash: &str, password: &str) -> Result<bool> {
    argon2::verify_encoded(hash, password.as_bytes())
}
```

## Authorization

### Role-Based Access

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Player,
    Moderator,
    Admin,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationContext {
    pub player_id: u64,
    pub roles: Vec<Role>,
    pub permissions: Vec<String>,
}

impl AuthorizationContext {
    pub fn has_role(&self, role: &Role) -> bool {
        self.roles.contains(role)
    }

    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(&permission.to_string())
    }
}
```

### Permission Checks

```rust
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
    Admin,
}
```

## Network Security

### Rate Limiting

```rust
pub struct RateLimiter {
    redis: RedisCache,
    limits: HashMap<String, RateLimit>,
}

pub struct RateLimit {
    pub max_requests: u32,
    pub window: Duration,
}

impl RateLimiter {
    pub async fn check(&self, key: &str, limit: &RateLimit) -> Result<bool> {
        let current = self.redis.incr(key).await?;
        if current == 1 {
            self.redis.expire(key, limit.window).await?;
        }
        Ok(current <= limit.max_requests as u64)
    }
}
```

### Message Size Limits

```rust
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024; // 1MB
pub const MAX_PAYLOAD_SIZE: usize = 1024 * 512;  // 512KB

pub fn validate_message_size(message: &[u8]) -> Result<()> {
    if message.len() > MAX_MESSAGE_SIZE {
        return Err(SecurityError::MessageTooLarge {
            size: message.len(),
            limit: MAX_MESSAGE_SIZE,
        });
    }
    Ok(())
}
```

### Connection Limits

```rust
pub struct ConnectionGuard {
    max_connections: usize,
    current_connections: AtomicUsize,
    rate_limiter: RateLimiter,
}

impl ConnectionGuard {
    pub fn can_accept(&self) -> bool {
        self.current_connections.load(Ordering::Relaxed) < self.max_connections
    }

    pub fn on_connect(&self, addr: SocketAddr) -> Result<()> {
        if !self.can_accept() {
            return Err(SecurityError::ConnectionLimitReached);
        }

        // Per-IP rate limit
        let key = format!("connect:{}", addr.ip());
        if !self.rate_limiter.check(&key, &RateLimit {
            max_requests: 10,
            window: Duration::from_secs(60),
        }) {
            return Err(SecurityError::RateLimited);
        }

        self.current_connections.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn on_disconnect(&self) {
        self.current_connections.fetch_sub(1, Ordering::Relaxed);
    }
}
```

## Plugin Security

### Capability-Based Access Control

```
Plugin requests: player.read
    │
    ▼
CapabilityManager checks:
    1. Plugin declared this permission? → No → DENY
    2. Runtime has capability? → No → DENY
    3. Policy allows? → No → DENY
    4. All checks pass → GRANT
```

### WASM Sandbox

```rust
pub struct WasmSandbox {
    memory_limit: usize,
    execution_limit: Duration,
    fuel_limit: u64,
}

impl WasmSandbox {
    pub fn new(config: &SandboxConfig) -> Self {
        Self {
            memory_limit: config.memory_limit,
            execution_limit: config.execution_limit,
            fuel_limit: config.fuel_limit,
        }
    }

    pub fn create_store(&self, state: WasmState) -> Store<WasmState> {
        let engine = Engine::new(&Config::new()
            .consume_fuel(true)
            .epoch_interruption(true)
        ).unwrap();

        let mut store = Store::new(&engine, state);
        store.limiter(|s| self.memory_limit as i64);
        store.set_fuel(self.fuel_limit).unwrap();
        store
    }
}
```

## Input Validation

### Command Validation

```rust
pub fn validate_command(command: &Command) -> Result<()> {
    // Validate command type
    if command.command_type.is_empty() || command.command_type.len() > 100 {
        return Err(ValidationError::InvalidCommandType);
    }

    // Validate payload size
    if command.payload.len() > MAX_PAYLOAD_SIZE {
        return Err(ValidationError::PayloadTooLarge);
    }

    // Validate timestamp (not too old or in the future)
    let now = timestamp_millis();
    if command.timestamp > now + 5000 || command.timestamp < now - 30000 {
        return Err(ValidationError::InvalidTimestamp);
    }

    Ok(())
}
```

### String Validation

```rust
pub fn validate_player_name(name: &str) -> Result<()> {
    if name.len() < 3 || name.len() > 20 {
        return Err(ValidationError::InvalidNameLength);
    }

    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(ValidationError::InvalidNameCharacters);
    }

    // Check for reserved words
    if RESERVED_NAMES.contains(&name.to_lowercase().as_str()) {
        return Err(ValidationError::ReservedName);
    }

    Ok(())
}
```

## Security Configuration

```toml
[security]
jwt_secret = "${JWT_SECRET}"
token_expiry = 3600
refresh_expiry = 86400

[security.rate_limiting]
enabled = true
max_connections_per_ip = 10
max_commands_per_second = 20
max_messages_per_minute = 1000

[security.sandbox]
memory_limit = "64MB"
execution_limit = "100ms"
fuel_limit = 1000000

[security.plugins]
require_signature = false  # Enable in production
allowed_plugins = ["character", "combat", "inventory", "auction"]
```

## Threat Model

| Threat | Mitigation |
|--------|-----------|
| Brute force login | Rate limiting, account lockout |
| Memory exhaustion | Per-plugin memory limits |
| CPU exhaustion | Fuel metering, execution timeouts |
| Packet flooding | Per-session rate limits |
| Invalid messages | Input validation, message size limits |
| Privilege escalation | Capability checks at every host function |
| Replay attacks | Timestamp validation, sequence numbers |
| Plugin escape | WASM sandbox, no raw system access |

## References

- [03-capability.md](03-capability.md) - Capability system
- [06-wasm.md](06-wasm.md) - WASM sandboxing
- [07-protocol.md](07-protocol.md) - Protocol security
