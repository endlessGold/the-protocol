use std::sync::Arc;

use anyhow::Result;
use bytes::{BufMut, BytesMut};
use clap::{Parser, Subcommand};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;

use protocol_network::NetworkManager;
use protocol_plugin::{PluginEngine, HostContext, SharedState};
use protocol_protocol::*;
use protocol_routing::CommandRouter;
use protocol_security::CapabilityManager;
use protocol_session::SessionManager;
use protocol_application::GameWorld;
use protocol_domain::Inventory;

#[derive(Parser)]
#[command(name = "runtime", about = "The Protocol - Cross-Platform Game Runtime")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Server {
        #[arg(short, long, default_value = "127.0.0.1:7770")]
        bind: String,

        #[arg(short, long, default_value = "./plugins")]
        plugins: String,
    },
    Client {
        #[arg(short, long, default_value = "127.0.0.1:7770")]
        server: String,
    },
    Gateway {
        #[arg(short, long, default_value = "127.0.0.1:7770")]
        bind: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    protocol_observability::init_logging();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server { bind, plugins } => run_server(&bind, &plugins).await,
        Commands::Client { server } => run_client(&server).await,
        Commands::Gateway { bind } => run_gateway(&bind).await,
    }
}

async fn run_server(bind: &str, plugin_dir: &str) -> Result<()> {
    tracing::info!("Starting The Protocol Runtime in SERVER mode on {}", bind);
    tracing::info!("This is a Cross-Platform Game Runtime, not just a server.");

    let session_manager = Arc::new(SessionManager::new(1000));
    let command_router = Arc::new(CommandRouter::new());
    let network = NetworkManager::new(bind, session_manager.clone(), command_router.clone()).await?;
    let capability_manager = CapabilityManager::new(protocol_security::RuntimeCapabilities::server());
    let game_world = Arc::new(RwLock::new(GameWorld::new()));

    // Initialize WASM plugin engine
    let shared_state = Arc::new(SharedState::new());
    let mut plugin_engine = PluginEngine::with_shared_state(plugin_dir, shared_state);

    // Discover and compile plugins
    match plugin_engine.discover() {
        Ok(manifests) => {
            for manifest in &manifests {
                tracing::info!("Discovered plugin: {} v{}", manifest.name, manifest.version);
                if let Err(e) = plugin_engine.compile(&manifest.name) {
                    tracing::error!("Failed to compile plugin {}: {}", manifest.name, e);
                    continue;
                }

                let context = HostContext {
                    player_id: 0,
                    room_id: 1,
                    plugin_name: manifest.name.clone(),
                };

                if let Err(e) = plugin_engine.instantiate(&manifest.name, context) {
                    tracing::error!("Failed to instantiate plugin {}: {}", manifest.name, e);
                    continue;
                }

                if let Err(e) = plugin_engine.initialize(&manifest.name) {
                    tracing::error!("Failed to initialize plugin {}: {}", manifest.name, e);
                    continue;
                }

                if let Err(e) = plugin_engine.enable(&manifest.name) {
                    tracing::error!("Failed to enable plugin {}: {}", manifest.name, e);
                    continue;
                }

                tracing::info!("Plugin {} loaded and enabled", manifest.name);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to discover plugins: {}", e);
        }
    }

    // Register built-in command handlers
    let gw = game_world.clone();
    let sm = session_manager.clone();
    command_router.register("look", Arc::new(LookHandler { game_world: gw, session_manager: sm }));

    let gw = game_world.clone();
    let sm = session_manager.clone();
    command_router.register("move", Arc::new(MoveHandler { game_world: gw, session_manager: sm }));

    let gw = game_world.clone();
    let sm = session_manager.clone();
    command_router.register("attack", Arc::new(AttackHandler { game_world: gw, session_manager: sm }));

    let gw = game_world.clone();
    let sm = session_manager.clone();
    command_router.register("inventory", Arc::new(InventoryHandler { game_world: gw, session_manager: sm }));

    let gw = game_world.clone();
    let sm = session_manager.clone();
    command_router.register("create_character", Arc::new(CreateCharacterHandler { game_world: gw, session_manager: sm }));

    tracing::info!("Server ready. Waiting for connections...");

    // Run the server
    network.accept_connections().await?;

    Ok(())
}

async fn run_client(server_addr: &str) -> Result<()> {
    tracing::info!("Starting The Protocol Runtime in CLIENT mode, connecting to {}", server_addr);

    use tokio::net::TcpStream;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = TcpStream::connect(server_addr).await?;
    stream.set_nodelay(true)?;

    let (mut reader, mut writer) = stream.into_split();

    // Send Hello
    let hello = Message::hello(ClientType::MUD, None);
    let codec = ProtocolCodec::new();
    let hello_bytes = codec.encode(&hello)?;
    writer.write_all(&hello_bytes).await?;

    // Read HelloAck
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let total_len = u32::from_be_bytes(len_buf) as usize;
    let mut frame = vec![0u8; total_len - 4];
    reader.read_exact(&mut frame).await?;

    let mut full_frame = bytes::BytesMut::with_capacity(4 + total_len);
    full_frame.put_slice(&len_buf);
    full_frame.put_slice(&frame);

    let mut buf = full_frame;
    let ack = ProtocolCodec::decode_simple(&mut buf)?.ok_or_else(|| anyhow::anyhow!("No response"))?;

    match ack.message_type {
        MessageType::HelloAck => {
            let hello_ack: HelloAck = rmp_serde::from_slice(&ack.payload)?;
            tracing::info!("Connected! Session ID: {}", hello_ack.session_id);
            println!("Connected to server. Session: {}", hello_ack.session_id);
            println!("Type 'help' for available commands.");
        }
        _ => {
            tracing::error!("Unexpected response");
            return Err(anyhow::anyhow!("Handshake failed"));
        }
    }

    // Interactive MUD loop
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    let mut input = String::new();

    loop {
        input.clear();
        print!("> ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let bytes_read = reader.read_line(&mut input).await?;
        if bytes_read == 0 {
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts[0].to_lowercase();

        let message = match cmd.as_str() {
            "quit" | "exit" => {
                println!("Goodbye!");
                break;
            }
            "help" => {
                println!("Available commands:");
                println!("  look              - Look around");
                println!("  move <direction>  - Move (north/south/east/west/up/down)");
                println!("  attack <target>   - Attack an NPC");
                println!("  inventory         - Check inventory");
                println!("  create <name> <class> - Create character");
                println!("  quit              - Disconnect");
                continue;
            }
            "look" => {
                let cmd = Command {
                    id: rand::random(),
                    command_type: "look".to_string(),
                    session_id: 0,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    payload: vec![],
                };
                Message::command(cmd)
            }
            "move" => {
                let direction = parts.get(1).unwrap_or(&"");
                let dir = match *direction {
                    "north" | "n" => protocol_protocol::Direction::North,
                    "south" | "s" => protocol_protocol::Direction::South,
                    "east" | "e" => protocol_protocol::Direction::East,
                    "west" | "w" => protocol_protocol::Direction::West,
                    "up" | "u" => protocol_protocol::Direction::Up,
                    "down" | "d" => protocol_protocol::Direction::Down,
                    _ => protocol_protocol::Direction::North,
                };
                let move_cmd = MoveCommand {
                    direction: dir,
                };
                let payload = rmp_serde::to_vec(&move_cmd)?;
                let cmd = Command {
                    id: rand::random(),
                    command_type: "move".to_string(),
                    session_id: 0,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    payload,
                };
                Message::command(cmd)
            }
            "attack" => {
                let target = parts.get(1).unwrap_or(&"");
                let cmd = Command {
                    id: rand::random(),
                    command_type: "attack".to_string(),
                    session_id: 0,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    payload: target.as_bytes().to_vec(),
                };
                Message::command(cmd)
            }
            "inventory" => {
                let cmd = Command {
                    id: rand::random(),
                    command_type: "inventory".to_string(),
                    session_id: 0,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    payload: vec![],
                };
                Message::command(cmd)
            }
            "create" => {
                let name = parts.get(1).unwrap_or(&"Hero");
                let class = parts.get(2).unwrap_or(&"warrior");
                let create_cmd = CreateCharacterCommand {
                    name: name.to_string(),
                    class: class.to_string(),
                };
                let payload = rmp_serde::to_vec(&create_cmd)?;
                let cmd = Command {
                    id: rand::random(),
                    command_type: "create_character".to_string(),
                    session_id: 0,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    payload,
                };
                Message::command(cmd)
            }
            _ => {
                println!("Unknown command. Type 'help' for available commands.");
                continue;
            }
        };

        // Send command
        let msg_bytes = codec.encode(&message)?;
        writer.write_all(&msg_bytes).await?;

        // Read response
        match reader.read_exact(&mut len_buf).await {
            Ok(_) => {
                let total_len = u32::from_be_bytes(len_buf) as usize;
                let mut frame = vec![0u8; total_len - 4];
                reader.read_exact(&mut frame).await?;

                let mut full_frame = bytes::BytesMut::with_capacity(4 + total_len);
                full_frame.put_slice(&len_buf);
                full_frame.put_slice(&frame);

                let mut buf = full_frame;
                if let Some(response) = ProtocolCodec::decode_simple(&mut buf)? {
                    match response.message_type {
                        MessageType::CommandResponse => {
                            let resp: CommandResponse = rmp_serde::from_slice(&response.payload)?;
                            if resp.success {
                                if let Ok(look_resp) = rmp_serde::from_slice::<LookResponse>(&resp.payload) {
                                    println!("\n=== {} ===", look_resp.room_name);
                                    println!("{}", look_resp.room_description);
                                    if !look_resp.exits.is_empty() {
                                        println!("\nExits: {}", look_resp.exits.join(", "));
                                    }
                                    if !look_resp.players.is_empty() {
                                        println!("\nPlayers here:");
                                        for p in &look_resp.players {
                                            println!("  {} (Level {})", p.name, p.level);
                                        }
                                    }
                                    if !look_resp.npcs.is_empty() {
                                        println!("\nNPCs here:");
                                        for n in &look_resp.npcs {
                                            println!("  {} (HP: {}/{})", n.name, n.hp, n.max_hp);
                                        }
                                    }
                                } else if let Ok(move_resp) = rmp_serde::from_slice::<MoveResponse>(&resp.payload) {
                                    println!("\nYou move to {}.", move_resp.room_name.unwrap_or_default());
                                    println!("{}", move_resp.room_description.unwrap_or_default());
                                } else if let Ok(attack_resp) = rmp_serde::from_slice::<AttackResponse>(&resp.payload) {
                                    println!("{}", attack_resp.message.unwrap_or_default());
                                } else if let Ok(inv_resp) = rmp_serde::from_slice::<InventoryResponse>(&resp.payload) {
                                    println!("\n=== Inventory ===");
                                    if inv_resp.items.is_empty() {
                                        println!("  Empty");
                                    } else {
                                        for item in &inv_resp.items {
                                            println!("  {} x{}", item.name, item.quantity);
                                        }
                                    }
                                    println!("Gold: {}", inv_resp.gold);
                                } else {
                                    let display = String::from_utf8_lossy(&resp.payload);
                                    println!("{}", display);
                                }
                            } else {
                                println!("Error: {}", resp.error.unwrap_or_else(|| "Unknown error".to_string()));
                            }
                        }
                        MessageType::Error => {
                            let error: ErrorResponse = rmp_serde::from_slice(&response.payload)?;
                            println!("Error: {}", error.message);
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                println!("Connection lost: {}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn run_gateway(bind: &str) -> Result<()> {
    tracing::info!("Starting The Protocol Runtime in GATEWAY mode on {}", bind);
    tracing::info!("Gateway mode - routing traffic between clients and servers.");

    println!("Gateway mode not yet implemented.");
    println!("This would route traffic between clients and game servers.");

    Ok(())
}

// Command Handlers

/// Resolve which character a session's commands should act on. Every
/// non-creation command needs this - there is no default/shared character;
/// a session must `create_character` first (see `CreateCharacterHandler`,
/// which binds the session to the new character via `SessionManager::set_player`).
fn resolve_character_id(
    session_manager: &SessionManager,
    session_id: u64,
) -> Result<u64, protocol_routing::RoutingError> {
    session_manager.get_player_id(session_id).ok_or_else(|| {
        protocol_routing::RoutingError::HandlerError(
            "No character for this session yet - use create_character first".to_string(),
        )
    })
}

struct LookHandler {
    game_world: Arc<RwLock<GameWorld>>,
    session_manager: Arc<SessionManager>,
}

#[async_trait::async_trait]
impl protocol_routing::CommandHandler for LookHandler {
    async fn handle(&self, _command: Command, session_id: u64) -> Result<CommandResponse, protocol_routing::RoutingError> {
        let character_id = resolve_character_id(&self.session_manager, session_id)?;

        let world = self.game_world.read().await;
        let room_id = world.get_character(character_id)
            .ok_or_else(|| protocol_routing::RoutingError::HandlerError("Character not found".to_string()))?
            .room_id;
        let room_info = world.look_room(room_id)
            .ok_or_else(|| protocol_routing::RoutingError::HandlerError("Room not found".to_string()))?;

        let response = LookResponse {
            room_name: room_info.name,
            room_description: room_info.description,
            exits: room_info.exits,
            players: room_info.players.into_iter().map(|p| protocol_protocol::PlayerSummary {
                id: p.id,
                name: p.name,
                level: p.level,
            }).collect(),
            npcs: room_info.npcs.into_iter().map(|n| protocol_protocol::NpcSummary {
                id: n.id,
                name: n.name,
                hp: n.hp,
                max_hp: n.max_hp,
            }).collect(),
        };

        let payload = rmp_serde::to_vec(&response)
            .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;

        Ok(CommandResponse {
            id: _command.id,
            command_type: "look".to_string(),
            success: true,
            payload,
            error: None,
        })
    }
}

struct MoveHandler {
    game_world: Arc<RwLock<GameWorld>>,
    session_manager: Arc<SessionManager>,
}

#[async_trait::async_trait]
impl protocol_routing::CommandHandler for MoveHandler {
    async fn handle(&self, command: Command, session_id: u64) -> Result<CommandResponse, protocol_routing::RoutingError> {
        let character_id = resolve_character_id(&self.session_manager, session_id)?;

        let move_cmd: MoveCommand = rmp_serde::from_slice(&command.payload)
            .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;

        let domain_dir = match move_cmd.direction {
            protocol_protocol::Direction::North => protocol_domain::Direction::North,
            protocol_protocol::Direction::South => protocol_domain::Direction::South,
            protocol_protocol::Direction::East => protocol_domain::Direction::East,
            protocol_protocol::Direction::West => protocol_domain::Direction::West,
            protocol_protocol::Direction::Up => protocol_domain::Direction::Up,
            protocol_protocol::Direction::Down => protocol_domain::Direction::Down,
        };

        let mut world = self.game_world.write().await;
        match world.move_character(character_id, domain_dir) {
            Ok(result) => {
                let response = MoveResponse {
                    success: true,
                    room_name: Some(result.room_name),
                    room_description: Some(result.room_description),
                    error: None,
                };
                let payload = rmp_serde::to_vec(&response)
                    .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;
                Ok(CommandResponse {
                    id: command.id,
                    command_type: "move".to_string(),
                    success: true,
                    payload,
                    error: None,
                })
            }
            Err(e) => {
                let response = MoveResponse {
                    success: false,
                    room_name: None,
                    room_description: None,
                    error: Some(e.to_string()),
                };
                let payload = rmp_serde::to_vec(&response)
                    .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;
                Ok(CommandResponse {
                    id: command.id,
                    command_type: "move".to_string(),
                    success: false,
                    payload,
                    error: Some(e.to_string()),
                })
            }
        }
    }
}

struct AttackHandler {
    game_world: Arc<RwLock<GameWorld>>,
    session_manager: Arc<SessionManager>,
}

#[async_trait::async_trait]
impl protocol_routing::CommandHandler for AttackHandler {
    async fn handle(&self, command: Command, session_id: u64) -> Result<CommandResponse, protocol_routing::RoutingError> {
        let character_id = resolve_character_id(&self.session_manager, session_id)?;
        let target_name = String::from_utf8_lossy(&command.payload).to_string();

        let mut world = self.game_world.write().await;
        match world.start_combat(character_id, &target_name) {
            Ok(combat_info) => {
                let response = AttackResponse {
                    success: true,
                    damage: Some(combat_info.damage),
                    target_hp: Some(combat_info.target_hp),
                    message: Some(combat_info.message),
                    error: None,
                };
                let payload = rmp_serde::to_vec(&response)
                    .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;
                Ok(CommandResponse {
                    id: command.id,
                    command_type: "attack".to_string(),
                    success: true,
                    payload,
                    error: None,
                })
            }
            Err(e) => {
                let response = AttackResponse {
                    success: false,
                    damage: None,
                    target_hp: None,
                    message: None,
                    error: Some(e.to_string()),
                };
                let payload = rmp_serde::to_vec(&response)
                    .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;
                Ok(CommandResponse {
                    id: command.id,
                    command_type: "attack".to_string(),
                    success: false,
                    payload,
                    error: Some(e.to_string()),
                })
            }
        }
    }
}

struct InventoryHandler {
    game_world: Arc<RwLock<GameWorld>>,
    session_manager: Arc<SessionManager>,
}

#[async_trait::async_trait]
impl protocol_routing::CommandHandler for InventoryHandler {
    async fn handle(&self, _command: Command, session_id: u64) -> Result<CommandResponse, protocol_routing::RoutingError> {
        let character_id = resolve_character_id(&self.session_manager, session_id)?;

        let world = self.game_world.read().await;
        let inventory = world.get_inventory(character_id).cloned().unwrap_or_else(|| Inventory::new());

        let response = InventoryResponse {
            items: inventory.items.into_iter().map(|i| protocol_protocol::InventoryItem {
                item_id: i.item_id,
                name: i.name,
                quantity: i.quantity,
                item_type: "item".to_string(),
            }).collect(),
            gold: inventory.gold,
        };

        let payload = rmp_serde::to_vec(&response)
            .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;

        Ok(CommandResponse {
            id: _command.id,
            command_type: "inventory".to_string(),
            success: true,
            payload,
            error: None,
        })
    }
}

struct CreateCharacterHandler {
    game_world: Arc<RwLock<GameWorld>>,
    session_manager: Arc<SessionManager>,
}

#[async_trait::async_trait]
impl protocol_routing::CommandHandler for CreateCharacterHandler {
    async fn handle(&self, command: Command, session_id: u64) -> Result<CommandResponse, protocol_routing::RoutingError> {
        let create_cmd: CreateCharacterCommand = rmp_serde::from_slice(&command.payload)
            .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;

        let mut world = self.game_world.write().await;
        match world.create_character(create_cmd.name, &create_cmd.class) {
            Ok(mut character) => {
                let character_id = character.id;
                world.add_character(character);

                // Bind this session to the character it just created, so
                // subsequent look/move/attack/inventory commands from this
                // session know which character to act on.
                self.session_manager
                    .set_player(session_id, character_id)
                    .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;

                let response = CreateCharacterResponse {
                    success: true,
                    character_id: Some(character_id),
                    error: None,
                };
                let payload = rmp_serde::to_vec(&response)
                    .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;
                Ok(CommandResponse {
                    id: command.id,
                    command_type: "create_character".to_string(),
                    success: true,
                    payload,
                    error: None,
                })
            }
            Err(e) => {
                let response = CreateCharacterResponse {
                    success: false,
                    character_id: None,
                    error: Some(e.to_string()),
                };
                let payload = rmp_serde::to_vec(&response)
                    .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;
                Ok(CommandResponse {
                    id: command.id,
                    command_type: "create_character".to_string(),
                    success: false,
                    payload,
                    error: Some(e.to_string()),
                })
            }
        }
    }
}
