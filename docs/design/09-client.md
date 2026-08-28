# 09 - Client Mode

## Overview

Client Mode uses the same Runtime Core to connect to a Runtime Server. The Runtime is not split into separate client and server programs - one binary serves both roles.

## Client Architecture

```
┌─────────────────────────────────────────────┐
│              Runtime (Client Mode)           │
│                                             │
│  ┌──────────┐ ┌──────────┐ ┌─────────────┐│
│  │ Config   │ │ Session  │ │ Plugin      ││
│  │          │ │ Manager  │ │ Runtime     ││
│  └──────────┘ └──────────┘ └─────────────┘│
│                                             │
│  ┌──────────────────────────────────────────┐│
│  │           Network Client                  ││
│  │  ┌──────┐ ┌──────┐ ┌──────────────────┐││
│  │  │ TCP  │ │ UDP  │ │ WebSocket Client │││
│  │  └──────┘ └──────┘ └──────────────────┘││
│  └──────────────────────────────────────────┘│
│                                             │
│  ┌──────────────────────────────────────────┐│
│  │           Application Layer              ││
│  │  ┌──────────┐ ┌──────────────────────┐  ││
│  │  │ Commands │ │ Event Handlers       │  ││
│  │  └──────────┘ └──────────────────────┘  ││
│  └──────────────────────────────────────────┘│
└─────────────────────────────────────────────┘
```

## Client Configuration

```toml
[runtime]
mode = "client"
name = "mud-client"

[client]
server_address = "127.0.0.1:7770"
transport = "tcp"  # tcp | udp | websocket
auto_reconnect = true
reconnect_interval = 5
max_reconnect_attempts = 10

[client.authentication]
type = "token"  # token | password
token = "your-auth-token"

[client.session]
heartbeat_interval = 30
timeout = 60
```

## Client Runtime

```rust
pub struct ClientRuntime {
    config: ClientConfig,
    connection: Option<Box<dyn Transport>>,
    session: ClientSession,
    plugin_runtime: Option<PluginRuntime>,
    command_handlers: HashMap<String, Box<dyn CommandHandler>>,
    event_handlers: HashMap<String, Box<dyn EventHandler>>,
}

impl ClientRuntime {
    pub async fn new(config: ClientConfig) -> Result<Self> {
        Ok(Self {
            connection: None,
            session: ClientSession::new(),
            plugin_runtime: None,
            command_handlers: HashMap::new(),
            event_handlers: HashMap::new(),
            config,
        })
    }

    pub async fn connect(&mut self) -> Result<()> {
        let transport: Box<dyn Transport> = match self.config.transport {
            TransportType::Tcp => {
                Box::new(TcpTransport::connect(&self.config.server_address).await?)
            }
            TransportType::Udp => {
                Box::new(UdpTransport::connect(&self.config.server_address).await?)
            }
            TransportType::WebSocket => {
                Box::new(WsTransport::connect(&self.config.server_address).await?)
            }
        };

        self.connection = Some(transport);

        // Perform handshake
        self.handshake().await?;

        // Start receive loop
        self.start_receive_loop().await;

        Ok(())
    }

    async fn handshake(&mut self) -> Result<()> {
        let hello = Message::hello(
            ClientType::Game,
            &self.config.authentication,
        );
        self.send(hello).await?;

        let response = self.recv().await?;
        match response.message_type {
            MessageType::HelloAck => {
                let ack: HelloAck = rmp_serde::from_slice(&response.payload)?;
                self.session.initialize(ack);
                Ok(())
            }
            MessageType::Error => {
                let error: ErrorResponse = rmp_serde::from_slice(&response.payload)?;
                Err(ClientError::HandshakeFailed(error.message))
            }
            _ => Err(ClientError::UnexpectedMessage),
        }
    }
}
```

## Command Sending

```rust
impl ClientRuntime {
    pub async fn send_command(&mut self, command_type: &str, payload: Vec<u8>) -> Result<CommandResponse> {
        let command = Command {
            id: self.session.next_message_id(),
            command_type: command_type.to_string(),
            session_id: self.session.id(),
            timestamp: timestamp_millis(),
            payload,
        };

        let message = Message::command(command);
        self.send(message).await?;

        // Wait for response with matching ID
        self.wait_for_response(command.id).await
    }

    // Convenience methods
    pub async fn login(&mut self, username: &str, password: &str) -> Result<LoginResponse> {
        let payload = rmp_serde::to_vec(&LoginCommand {
            username: username.to_string(),
            password: password.to_string(),
        })?;
        let response = self.send_command("login", payload).await?;
        rmp_serde::from_slice(&response.payload)
    }

    pub async fn look(&mut self) -> Result<RoomDescription> {
        let response = self.send_command("look", vec![]).await?;
        rmp_serde::from_slice(&response.payload)
    }

    pub async fn move_dir(&mut self, direction: Direction) -> Result<MoveResponse> {
        let payload = rmp_serde::to_vec(&MoveCommand { direction })?;
        let response = self.send_command("move", payload).await?;
        rmp_serde::from_slice(&response.payload)
    }

    pub async fn attack(&mut self, target_id: u64) -> Result<AttackResponse> {
        let payload = rmp_serde::to_vec(&AttackCommand {
            target_id,
            weapon_id: None,
        })?;
        let response = self.send_command("attack", payload).await?;
        rmp_serde::from_slice(&response.payload)
    }

    pub async fn inventory(&mut self) -> Result<InventoryResponse> {
        let response = self.send_command("inventory", vec![]).await?;
        rmp_serde::from_slice(&response.payload)
    }
}
```

## Event Receiving

```rust
impl ClientRuntime {
    async fn start_receive_loop(&mut self) {
        let connection = self.connection.as_ref().unwrap();
        let mut recv = connection.recv_stream();

        while let Some(message) = recv.next().await {
            match message.message_type {
                MessageType::Event => {
                    self.handle_event(message).await;
                }
                MessageType::Pong => {
                    self.session.on_pong();
                }
                MessageType::Disconnect => {
                    self.handle_disconnect().await;
                    break;
                }
                _ => {}
            }
        }
    }

    async fn handle_event(&mut self, message: Message) {
        let event: Event = match rmp_serde::from_slice(&message.payload) {
            Ok(e) => e,
            Err(_) => return,
        };

        // Notify event handlers
        if let Some(handler) = self.event_handlers.get(&event.event_type) {
            handler.handle(&event).await;
        }

        // Emit to plugin runtime
        if let Some(ref mut plugin_runtime) = self.plugin_runtime {
            plugin_runtime.handle_event(&event).await;
        }
    }
}
```

## MUD Client Integration

```rust
pub struct MudClient {
    runtime: ClientRuntime,
    ui: TerminalUI,
}

impl MudClient {
    pub async fn new(config: ClientConfig) -> Result<Self> {
        let runtime = ClientRuntime::new(config).await?;
        let ui = TerminalUI::new()?;

        Ok(Self { runtime, ui })
    }

    pub async fn run(&mut self) -> Result<()> {
        self.runtime.connect().await?;

        self.ui.print("Connected to server. Type 'help' for commands.\n");

        loop {
            let input = self.ui.read_input().await?;

            match self.parse_command(&input) {
                Some(ClientCommand::Login(user, pass)) => {
                    match self.runtime.login(&user, &pass).await {
                        Ok(resp) => self.ui.print(&format!("Welcome, {}!\n", resp.character_name)),
                        Err(e) => self.ui.print(&format!("Login failed: {}\n", e)),
                    }
                }
                Some(ClientCommand::Look) => {
                    match self.runtime.look().await {
                        Ok(room) => self.ui.print_room(&room),
                        Err(e) => self.ui.print(&format!("Error: {}\n", e)),
                    }
                }
                Some(ClientCommand::Move(dir)) => {
                    match self.runtime.move_dir(dir).await {
                        Ok(_) => self.runtime.look().await.map(|room| self.ui.print_room(&room)),
                        Err(e) => self.ui.print(&format!("Cannot go that way: {}\n", e)),
                    }
                }
                Some(ClientCommand::Attack(target)) => {
                    match self.runtime.attack(target).await {
                        Ok(result) => self.ui.print_combat(&result),
                        Err(e) => self.ui.print(&format!("Attack failed: {}\n", e)),
                    }
                }
                Some(ClientCommand::Inventory) => {
                    match self.runtime.inventory().await {
                        Ok(inv) => self.ui.print_inventory(&inv),
                        Err(e) => self.ui.print(&format!("Error: {}\n", e)),
                    }
                }
                Some(ClientCommand::Quit) => break,
                None => self.ui.print("Unknown command.\n"),
            }
        }

        Ok(())
    }
}
```

## Auto-Reconnection

```rust
impl ClientRuntime {
    pub async fn run_with_reconnect(&mut self) -> Result<()> {
        let mut attempts = 0;

        loop {
            match self.connect().await {
                Ok(()) => {
                    attempts = 0;
                    self.run_event_loop().await?;
                }
                Err(e) => {
                    tracing::error!("Connection failed: {}", e);
                    attempts += 1;

                    if !self.config.auto_reconnect || attempts >= self.config.max_reconnect_attempts {
                        return Err(e);
                    }

                    tracing::info!("Reconnecting in {}s (attempt {}/{})",
                        self.config.reconnect_interval, attempts, self.config.max_reconnect_attempts);
                    tokio::time::sleep(Duration::from_secs(self.config.reconnect_interval)).await;
                }
            }
        }
    }
}
```

## Client-Server Communication Flow

```
Client                              Server
  │                                    │
  │──── Hello (version, auth) ───────→│
  │←── HelloAck (session, caps) ──────│
  │                                    │
  │──── Command (login) ─────────────→│
  │                                    │ [Plugin: character.login()]
  │←── CommandResponse (success) ─────│
  │                                    │
  │──── Command (look) ──────────────→│
  │                                    │ [Plugin: world.look()]
  │←── CommandResponse (room data) ───│
  │                                    │
  │←── Event (player_entered) ────────│ [Broadcast to room]
  │                                    │
  │──── Command (move north) ────────→│
  │                                    │ [Plugin: world.move()]
  │←── CommandResponse (new room) ────│
  │                                    │
  │←── Event (player_left) ───────────│ [Previous room]
  │←── Event (player_entered) ────────│ [New room]
```

## References

- [10-server-mode.md](10-server-mode.md) - Server mode (counterpart)
- [08-network.md](08-network.md) - Network transport
- [07-protocol.md](07-protocol.md) - Message format
