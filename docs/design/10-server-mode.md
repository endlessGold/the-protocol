# 10 - Server Mode

## Overview

Server Mode is one of several modes the Runtime can operate in. It is NOT the identity of the Runtime - it is a configuration choice that activates server capabilities.

## Server Mode = Runtime + Server Capability

```
Runtime (generic)
    + TCP Listener
    + UDP Listener
    + HTTP Server
    + Session Manager
    + Plugin Runtime
    + World State
    ─────────────────
    = Server Mode
```

## Server Configuration

```toml
[runtime]
mode = "server"
name = "game-world-1"

[server]
bind_address = "0.0.0.0:7770"
max_connections = 1000
tick_rate = 20  # Hz

[server.world]
name = "Aetherius"
max_players = 500
start_room = 1

[plugins]
directory = "./plugins"
allowed = ["character", "combat", "inventory", "auction"]
```

## Server Architecture

```
┌───────────────────────────────────────────────────┐
│                 Runtime (Server Mode)              │
│                                                   │
│  ┌──────────────────────────────────────────────┐ │
│  │              Network Layer                    │ │
│  │  ┌────────┐ ┌────────┐ ┌──────────────────┐ │ │
│  │  │  TCP   │ │  UDP   │ │ HTTP (Axum)      │ │ │
│  │  │:7770   │ │:7771   │ │:8080             │ │ │
│  │  └────────┘ └────────┘ └──────────────────┘ │ │
│  └──────────────────────────────────────────────┘ │
│                                                   │
│  ┌──────────────────────────────────────────────┐ │
│  │              Session Manager                  │ │
│  │  Track all connected clients, manage state   │ │
│  └──────────────────────────────────────────────┘ │
│                                                   │
│  ┌──────────────────────────────────────────────┐ │
│  │              Command Router                   │ │
│  │  Route commands to plugin handlers           │ │
│  └──────────────────────────────────────────────┘ │
│                                                   │
│  ┌──────────────────────────────────────────────┐ │
│  │              Plugin Runtime (WASM)            │ │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────────┐ │ │
│  │  │character │ │ combat   │ │ inventory    │ │ │
│  │  │.wasm     │ │ .wasm    │ │ .wasm        │ │ │
│  │  └──────────┘ └──────────┘ └──────────────┘ │ │
│  └──────────────────────────────────────────────┘ │
│                                                   │
│  ┌──────────────────────────────────────────────┐ │
│  │              World State                      │ │
│  │  Rooms, Players, Items, NPCs                 │ │
│  └──────────────────────────────────────────────┘ │
│                                                   │
│  ┌──────────────────────────────────────────────┐ │
│  │              Event Bus                        │ │
│  │  Internal events, broadcasting               │ │
│  └──────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────┘
```

## Server Runtime

```rust
pub struct ServerRuntime {
    config: ServerConfig,
    network: NetworkManager,
    session_manager: Arc<SessionManager>,
    plugin_runtime: PluginRuntime,
    command_router: CommandRouter,
    event_bus: EventBus,
    world: WorldState,
    tick_rate: u32,
}

impl ServerRuntime {
    pub async fn new(config: RuntimeConfig) -> Result<Self> {
        let session_manager = Arc::new(SessionManager::new(&config.server));
        let network = NetworkManager::new(&config.network, session_manager.clone()).await?;
        let plugin_runtime = PluginRuntime::new(&config.plugins).await?;
        let command_router = CommandRouter::new();
        let event_bus = EventBus::new();
        let world = WorldState::new(&config.server.world);

        Ok(Self {
            config: config.server.clone(),
            network,
            session_manager,
            plugin_runtime,
            command_router,
            event_bus,
            world,
            tick_rate: config.server.tick_rate,
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        // Load plugins
        self.plugin_runtime.load_all().await?;

        // Register plugin commands and events
        self.register_plugin_routes().await?;

        // Start game loop
        self.run_game_loop().await
    }

    async fn run_game_loop(&mut self) -> Result<()> {
        let tick_duration = Duration::from_millis(1000 / self.tick_rate as u64);
        let mut interval = tokio::time::interval(tick_duration);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.tick().await?;
                }
                Some(connection) = self.network.accept() => {
                    self.handle_new_connection(connection).await?;
                }
                Some(msg) = self.event_bus.recv() => {
                    self.handle_event(msg).await?;
                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }

        self.shutdown().await
    }

    async fn tick(&mut self) -> Result<()> {
        // Update world state
        self.world.update()?;

        // Process scheduled tasks
        self.plugin_runtime.tick().await?;

        // Broadcast world updates to clients
        self.broadcast_updates().await?;

        Ok(())
    }
}
```

## Command Router

```rust
pub struct CommandRouter {
    routes: HashMap<String, Box<dyn CommandHandler>>,
}

impl CommandRouter {
    pub fn new() -> Self {
        Self { routes: HashMap::new() }
    }

    pub fn register(&mut self, command_type: &str, handler: Box<dyn CommandHandler>) {
        self.routes.insert(command_type.to_string(), handler);
    }

    pub async fn route(&self, command: Command, session: &Session) -> Result<CommandResponse> {
        let handler = self.routes.get(&command.command_type)
            .ok_or_else(|| RouterError::UnknownCommand(command.command_type.clone()))?;

        handler.handle(command, session).await
    }
}

#[async_trait]
pub trait CommandHandler: Send + Sync {
    async fn handle(&self, command: Command, session: &Session) -> Result<CommandResponse>;
}
```

## Session Manager (Server Side)

```rust
pub struct SessionManager {
    sessions: DashMap<u64, Session>,
    address_sessions: DashMap<SocketAddr, u64>,
    config: ServerConfig,
}

pub struct Session {
    pub id: u64,
    pub player_id: Option<u64>,
    pub address: SocketAddr,
    pub transport: TransportType,
    pub state: SessionState,
    pub connected_at: Instant,
    pub last_activity: Instant,
    pub outgoing_tx: mpsc::Sender<Message>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Connected,
    Authenticating,
    Authenticated,
    InGame,
    Disconnected,
}

impl SessionManager {
    pub async fn create_tcp_session(&self, addr: SocketAddr) -> Result<u64> {
        let session_id = self.next_id();
        let (tx, rx) = mpsc::channel(100);

        let session = Session {
            id: session_id,
            player_id: None,
            address: addr,
            transport: TransportType::Tcp,
            state: SessionState::Connected,
            connected_at: Instant::now(),
            last_activity: Instant::now(),
            outgoing_tx: tx,
        };

        self.sessions.insert(session_id, session);
        self.address_sessions.insert(addr, session_id);

        tracing::info!("New TCP session {} from {}", session_id, addr);
        Ok(session_id)
    }

    pub async fn route_message(&self, session_id: u64, message: Message) -> Result<()> {
        let session = self.sessions.get(&session_id)
            .ok_or(SessionError::NotFound(session_id))?;

        match message.message_type {
            MessageType::Command => {
                let command: Command = rmp_serde::from_slice(&message.payload)?;
                // Route to command handler
            }
            MessageType::Ping => {
                // Send Pong
            }
            _ => {}
        }

        Ok(())
    }

    pub async fn broadcast(&self, event: &Event, room_id: Option<u32>) {
        let data = rmp_serde::to_vec(event).unwrap();
        let message = Message::event(event.clone());

        for session in self.sessions.iter() {
            if let Some(rid) = room_id {
                // Only send to sessions in the same room
                if !session.is_in_room(rid) { continue; }
            }
            let _ = session.outgoing_tx.send(message.clone()).await;
        }
    }
}
```

## World State

```rust
pub struct WorldState {
    rooms: HashMap<u32, Room>,
    players: HashMap<u64, Player>,
    items: HashMap<u32, Item>,
    npcs: HashMap<u64, NPC>,
    config: WorldConfig,
}

pub struct Room {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub exits: HashMap<Direction, u32>,
    pub players: Vec<u64>,
    pub npcs: Vec<u64>,
    pub items: Vec<u32>,
}

pub struct Player {
    pub id: u64,
    pub name: String,
    pub room_id: u32,
    pub hp: u32,
    pub max_hp: u32,
    pub level: u32,
    pub inventory: Vec<ItemStack>,
    pub equipment: Equipment,
}

impl WorldState {
    pub fn new(config: &WorldConfig) -> Self {
        let mut world = Self {
            rooms: HashMap::new(),
            players: HashMap::new(),
            items: HashMap::new(),
            npcs: HashMap::new(),
            config: config.clone(),
        };

        world.initialize_world();
        world
    }

    fn initialize_world(&mut self) {
        // Create starting rooms
        self.rooms.insert(1, Room {
            id: 1,
            name: "Town Square".to_string(),
            description: "A bustling town square with a fountain in the center.".to_string(),
            exits: HashMap::from([
                (Direction::North, 2),
                (Direction::East, 3),
            ]),
            players: Vec::new(),
            npcs: Vec::new(),
            items: Vec::new(),
        });

        // More rooms...
    }

    pub fn move_player(&mut self, player_id: u64, direction: Direction) -> Result<u32> {
        let player = self.players.get(&player_id)
            .ok_or(WorldError::PlayerNotFound(player_id))?;

        let current_room = self.rooms.get(&player.room_id)
            .ok_or(WorldError::RoomNotFound(player.room_id))?;

        let new_room_id = current_room.exits.get(&direction)
            .ok_or(WorldError::NoExit(direction))?
            .clone();

        // Remove from old room
        if let Some(room) = self.rooms.get_mut(&player.room_id) {
            room.players.retain(|&id| id != player_id);
        }

        // Add to new room
        if let Some(room) = self.rooms.get_mut(&new_room_id) {
            room.players.push(player_id);
        }

        // Update player position
        self.players.get_mut(&player_id).unwrap().room_id = new_room_id;

        Ok(new_room_id)
    }
}
```

## Game Loop Timing

```
┌──────────────────────────────────────────┐
│              Game Loop (20 Hz)            │
│                                          │
│  Tick 1: Process inputs, update world    │
│  Tick 2: Process inputs, update world    │
│  ...                                     │
│  Tick 20: Process inputs, update world   │
│                                          │
│  Total: 20 ticks per second              │
│  Each tick: ~50ms budget                 │
└──────────────────────────────────────────┘
```

## Server Statistics

```rust
pub struct ServerStats {
    pub uptime: Duration,
    pub total_connections: u64,
    pub active_sessions: usize,
    pub total_commands: u64,
    pub commands_per_second: f64,
    pub active_plugins: usize,
    pub world_state: WorldStats,
}

impl ServerRuntime {
    pub fn stats(&self) -> ServerStats {
        ServerStats {
            uptime: self.start_time.elapsed(),
            total_connections: self.session_manager.total_connected(),
            active_sessions: self.session_manager.count(),
            total_commands: self.command_router.total_routed(),
            commands_per_second: self.command_router.commands_per_second(),
            active_plugins: self.plugin_runtime.enabled_count(),
            world_state: self.world.stats(),
        }
    }
}
```

## References

- [09-client.md](09-client.md) - Client mode (counterpart)
- [08-network.md](08-network.md) - Network layer
- [11-gateway.md](11-gateway.md) - Gateway mode
- [12-domain.md](12-domain.md) - World state entities
