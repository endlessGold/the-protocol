# 11 - Gateway Mode

## Overview

Gateway Mode is one of several Runtime modes. The Gateway routes traffic between clients and game servers, handles authentication, load balancing, and server discovery.

## Gateway = Runtime + Gateway Capability

```
Runtime (generic)
    + TCP Listener
    + UDP Listener
    + HTTP Server
    + Authentication
    + Routing
    + Load Balancing
    + Session Management
    ─────────────────
    = Gateway Mode
```

## Gateway Architecture

```
                                    ┌─────────────────┐
                                    │    Gateway       │
                                    │   (Runtime)      │
                                    │                  │
Clients ────────────────────────────┤  Authentication  │
                                    │  Routing         │
                                    │  Load Balancing  │
                                    │  Session Cache   │
                                    └────────┬────────┘
                                             │
                          ┌──────────────────┼──────────────────┐
                          │                  │                  │
                    ┌─────▼─────┐      ┌─────▼─────┐     ┌─────▼─────┐
                    │ Runtime   │      │ Runtime   │     │ Runtime   │
                    │ Server #1 │      │ Server #2 │     │ Server #3 │
                    │ (Zone A)  │      │ (Zone B)  │     │ (Zone C)  │
                    └───────────┘      └───────────┘     └───────────┘
```

## Gateway Configuration

```toml
[runtime]
mode = "gateway"
name = "main-gateway"

[gateway]
bind_address = "0.0.0.0:7770"
max_connections = 10000

[gateway.authentication]
type = "jwt"
secret = "${JWT_SECRET}"
token_expiry = 3600

[gateway.routing]
# Static routes (fallback)
[[gateway.routing.static]]
pattern = "zone:*"
target = "zone-servers"

# Dynamic routes (from service discovery)
[gateway.routing.discovery]
enabled = true
method = "dns"  # dns | consul | static

[gateway.load_balancing]
algorithm = "round_robin"  # round_robin | least_connections | weighted
health_check_interval = 10
health_check_timeout = 5

[gateway.servers]
[[gateway.servers.instances]]
id = "zone-1"
address = "127.0.0.1:7771"
zone = "starter"
weight = 1
max_players = 500

[[gateway.servers.instances]]
id = "zone-2"
address = "127.0.0.1:7772"
zone = "dungeon"
weight = 1
max_players = 200
```

## Gateway Runtime

```rust
pub struct GatewayRuntime {
    config: GatewayConfig,
    network: NetworkManager,
    session_manager: Arc<GatewaySessionManager>,
    auth_manager: AuthManager,
    router: GatewayRouter,
    load_balancer: LoadBalancer,
    server_registry: ServerRegistry,
    event_bus: EventBus,
}

impl GatewayRuntime {
    pub async fn new(config: RuntimeConfig) -> Result<Self> {
        let session_manager = Arc::new(GatewaySessionManager::new());
        let network = NetworkManager::new(&config.network, session_manager.clone()).await?;
        let auth_manager = AuthManager::new(&config.gateway.authentication)?;
        let router = GatewayRouter::new(&config.gateway.routing)?;
        let load_balancer = LoadBalancer::new(&config.gateway.load_balancing);
        let server_registry = ServerRegistry::new(&config.gateway.servers);

        Ok(Self {
            config: config.gateway.clone(),
            network,
            session_manager,
            auth_manager,
            router,
            load_balancer,
            server_registry,
            event_bus: EventBus::new(),
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        // Start health checks
        self.start_health_checks().await?;

        // Start gateway event loop
        self.run_gateway_loop().await
    }

    async fn run_gateway_loop(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(connection) = self.network.accept() => {
                    self.handle_client_connection(connection).await?;
                }
                Some(msg) = self.event_bus.recv() => {
                    self.handle_event(msg).await?;
                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
        Ok(())
    }
}
```

## Authentication

```rust
pub struct AuthManager {
    jwt_secret: String,
    token_expiry: Duration,
}

impl AuthManager {
    pub fn new(config: &AuthConfig) -> Result<Self> {
        Ok(Self {
            jwt_secret: config.secret.clone(),
            token_expiry: Duration::from_secs(config.token_expiry),
        })
    }

    pub async fn authenticate(&self, message: &Message) -> Result<AuthResult> {
        match message.message_type {
            MessageType::Hello => {
                let hello: Hello = rmp_serde::from_slice(&message.payload)?;

                if let Some(ref token) = hello.auth_token {
                    // Validate JWT
                    let claims = self.validate_jwt(token)?;
                    Ok(AuthResult::Authenticated {
                        player_id: claims.player_id,
                        permissions: claims.permissions,
                    })
                } else {
                    Ok(AuthResult::RequiresAuth)
                }
            }
            _ => Ok(AuthResult::NotAuthenticated),
        }
    }

    fn validate_jwt(&self, token: &str) -> Result<Claims> {
        let token_data = jsonwebtoken::decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &Validation::default(),
        )?;
        Ok(token_data.claims)
    }

    pub fn create_token(&self, player_id: u64, permissions: Vec<String>) -> Result<String> {
        let claims = Claims {
            player_id,
            permissions,
            exp: chrono::Utc::now() + self.token_expiry,
            iat: chrono::Utc::now(),
        };

        let token = jsonwebtoken::encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )?;

        Ok(token)
    }
}
```

## Gateway Router

```rust
pub struct GatewayRouter {
    routes: Vec<Route>,
    server_routes: HashMap<String, String>,  // server_id -> zone
}

#[derive(Debug, Clone)]
pub struct Route {
    pub pattern: RoutePattern,
    pub target: RouteTarget,
}

#[derive(Debug, Clone)]
pub enum RoutePattern {
    Zone(String),       // "zone:starter"
    Player(u64),        // "player:12345"
    Service(String),    // "service:auction"
    Default,
}

#[derive(Debug, Clone)]
pub enum RouteTarget {
    Server(String),     // server_id
    LoadBalanced(Vec<String>),  // list of server_ids
    Reject,
}

impl GatewayRouter {
    pub fn resolve(&self, session: &GatewaySession, command: &str) -> Result<RouteTarget> {
        // 1. Check if session has assigned server
        if let Some(ref server_id) = session.assigned_server {
            return Ok(RouteTarget::Server(server_id.clone()));
        }

        // 2. Check static routes
        for route in &self.routes {
            if route.pattern.matches(session, command) {
                return Ok(route.target.clone());
            }
        }

        // 3. Default: load balanced
        let available = self.server_routes.keys().cloned().collect();
        Ok(RouteTarget::LoadBalanced(available))
    }
}
```

## Load Balancer

```rust
pub struct LoadBalancer {
    algorithm: LoadBalanceAlgorithm,
    server_stats: HashMap<String, ServerStats>,
}

#[derive(Debug, Clone)]
pub enum LoadBalanceAlgorithm {
    RoundRobin,
    LeastConnections,
    Weighted,
}

pub struct ServerStats {
    pub id: String,
    pub active_connections: usize,
    pub max_connections: usize,
    pub weight: u32,
    pub healthy: bool,
    pub last_health_check: Instant,
}

impl LoadBalancer {
    pub fn select_server(&self, candidates: &[String]) -> Result<String> {
        let healthy: Vec<&ServerStats> = candidates.iter()
            .filter_map(|id| self.server_stats.get(id))
            .filter(|s| s.healthy)
            .collect();

        if healthy.is_empty() {
            return Err(GatewayError::NoHealthyServers);
        }

        match self.algorithm {
            LoadBalanceAlgorithm::RoundRobin => {
                // Simple round-robin
                let idx = self.next_index.fetch_add(1, Ordering::Relaxed) % healthy.len();
                Ok(healthy[idx].id.clone())
            }
            LoadBalanceAlgorithm::LeastConnections => {
                healthy.iter()
                    .min_by_key(|s| s.active_connections)
                    .map(|s| s.id.clone())
                    .ok_or(GatewayError::NoHealthyServers)
            }
            LoadBalanceAlgorithm::Weighted => {
                // Weighted random selection
                let total_weight: u32 = healthy.iter().map(|s| s.weight).sum();
                let mut rng = thread_rng();
                let mut roll = rng.gen_range(0..total_weight);

                for server in &healthy {
                    if roll < server.weight {
                        return Ok(server.id.clone());
                    }
                    roll -= server.weight;
                }

                Ok(healthy.last().unwrap().id.clone())
            }
        }
    }
}
```

## Server Health Checks

```rust
impl GatewayRuntime {
    async fn start_health_checks(&self) -> Result<()> {
        let interval = Duration::from_secs(self.config.load_balancing.health_check_interval);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                self.check_server_health().await;
            }
        });

        Ok(())
    }

    async fn check_server_health(&self) {
        for server in self.server_registry.get_all() {
            let healthy = self.ping_server(&server.address).await;
            self.load_balancer.update_health(&server.id, healthy);
        }
    }

    async fn ping_server(&self, address: &str) -> bool {
        match tokio::time::timeout(
            Duration::from_secs(self.config.load_balancing.health_check_timeout),
            TcpStream::connect(address),
        ).await {
            Ok(Ok(_)) => true,
            _ => false,
        }
    }
}
```

## Gateway Session

```rust
pub struct GatewaySession {
    pub id: u64,
    pub player_id: Option<u64>,
    pub address: SocketAddr,
    pub state: GatewaySessionState,
    pub assigned_server: Option<String>,
    pub connected_at: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GatewaySessionState {
    Connected,
    Authenticating,
    Authenticated,
    InGame,
    Transferring,
}
```

## Server Transfer

```rust
impl GatewayRuntime {
    pub async fn transfer_player(
        &self,
        player_id: u64,
        from_server: &str,
        to_server: &str,
    ) -> Result<()> {
        // 1. Get player state from source server
        let player_state = self.get_player_state(from_server, player_id).await?;

        // 2. Send player state to destination server
        self.send_player_state(to_server, &player_state).await?;

        // 3. Update routing table
        self.router.update_player_route(player_id, to_server)?;

        // 4. Notify client of transfer
        self.notify_transfer(player_id, to_server).await?;

        Ok(())
    }
}
```

## Gateway Statistics

```rust
pub struct GatewayStats {
    pub uptime: Duration,
    pub total_connections: u64,
    pub active_sessions: usize,
    pub total_transfers: u64,
    pub servers_online: usize,
    pub servers_offline: usize,
    pub connections_per_second: f64,
    pub avg_latency_ms: f64,
}
```

## References

- [10-server-mode.md](10-server-mode.md) - Server mode
- [09-client.md](09-client.md) - Client mode
- [14-distributed.md](14-distributed.md) - Distributed runtime
