use std::sync::Arc;

use anyhow::Result;
use bytes::BufMut;
use clap::{Parser, Subcommand};
use tokio::io::AsyncBufReadExt;
use tokio::sync::RwLock;

use protocol_application::GameWorld;
use protocol_domain::Inventory;
use protocol_network::NetworkManager;
use protocol_plugin::{HostContext, PluginEngine, SharedState};
use protocol_protocol::*;
use protocol_routing::CommandRouter;
use protocol_security::CapabilityManager;
use protocol_session::SessionManager;

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
    let network =
        NetworkManager::new(bind, session_manager.clone(), command_router.clone()).await?;
    let capability_manager =
        CapabilityManager::new(protocol_security::RuntimeCapabilities::server());
    let game_world = Arc::new(RwLock::new(GameWorld::new()));

    // Initialize WASM plugin engine, if this runtime profile allows one.
    // This is the first thing that actually consults CapabilityManager -
    // it was previously constructed and never read, and its
    // has_runtime_capability() returned true unconditionally, so the whole
    // capability system was decorative.
    let mut plugin_engine = if capability_manager.has_runtime_capability("plugin_runtime") {
        let shared_state = Arc::new(SharedState::new());
        Some(PluginEngine::with_shared_state(plugin_dir, shared_state))
    } else {
        tracing::info!("plugin_runtime capability is off for this profile; skipping plugins");
        None
    };

    // Discover and compile plugins
    if let Some(engine) = plugin_engine.as_mut() {
        match engine.discover() {
            Ok(manifests) => {
                for manifest in &manifests {
                    tracing::info!("Discovered plugin: {} v{}", manifest.name, manifest.version);
                    if let Err(e) = engine.compile(&manifest.name) {
                        tracing::error!("Failed to compile plugin {}: {}", manifest.name, e);
                        continue;
                    }

                    let context = HostContext {
                        player_id: 0,
                        room_id: 1,
                        plugin_name: manifest.name.clone(),
                    };

                    if let Err(e) = engine.instantiate(&manifest.name, context) {
                        tracing::error!("Failed to instantiate plugin {}: {}", manifest.name, e);
                        continue;
                    }

                    if let Err(e) = engine.initialize(&manifest.name) {
                        tracing::error!("Failed to initialize plugin {}: {}", manifest.name, e);
                        continue;
                    }

                    if let Err(e) = engine.enable(&manifest.name) {
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
    }

    // Register built-in command handlers
    let gw = game_world.clone();
    let sm = session_manager.clone();
    command_router.register(
        "look",
        Arc::new(LookHandler {
            game_world: gw,
            session_manager: sm,
        }),
    );

    let gw = game_world.clone();
    let sm = session_manager.clone();
    command_router.register(
        "move",
        Arc::new(MoveHandler {
            game_world: gw,
            session_manager: sm,
        }),
    );

    let gw = game_world.clone();
    let sm = session_manager.clone();
    command_router.register(
        "attack",
        Arc::new(AttackHandler {
            game_world: gw,
            session_manager: sm,
        }),
    );

    let gw = game_world.clone();
    let sm = session_manager.clone();
    command_router.register(
        "inventory",
        Arc::new(InventoryHandler {
            game_world: gw,
            session_manager: sm,
        }),
    );

    let gw = game_world.clone();
    let sm = session_manager.clone();
    command_router.register(
        "create_character",
        Arc::new(CreateCharacterHandler {
            game_world: gw,
            session_manager: sm,
        }),
    );

    tracing::info!("Server ready. Waiting for connections...");

    // Run the server
    network.accept_connections().await?;

    Ok(())
}

async fn run_client(server_addr: &str) -> Result<()> {
    tracing::info!(
        "Starting The Protocol Runtime in CLIENT mode, connecting to {}",
        server_addr
    );

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let stream = TcpStream::connect(server_addr).await?;
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
    let ack =
        ProtocolCodec::decode_simple(&mut buf)?.ok_or_else(|| anyhow::anyhow!("No response"))?;

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
                let move_cmd = MoveCommand { direction: dir };
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
                                if let Ok(look_resp) =
                                    rmp_serde::from_slice::<LookResponse>(&resp.payload)
                                {
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
                                } else if let Ok(move_resp) =
                                    rmp_serde::from_slice::<MoveResponse>(&resp.payload)
                                {
                                    println!(
                                        "\nYou move to {}.",
                                        move_resp.room_name.unwrap_or_default()
                                    );
                                    println!("{}", move_resp.room_description.unwrap_or_default());
                                } else if let Ok(attack_resp) =
                                    rmp_serde::from_slice::<AttackResponse>(&resp.payload)
                                {
                                    println!("{}", attack_resp.message.unwrap_or_default());
                                } else if let Ok(inv_resp) =
                                    rmp_serde::from_slice::<InventoryResponse>(&resp.payload)
                                {
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
                                println!(
                                    "Error: {}",
                                    resp.error.unwrap_or_else(|| "Unknown error".to_string())
                                );
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

/// Translate `DomainEvent`s produced by a `GameWorld` call into
/// `PresentationCommand`s (see `protocol_presentation::translate_event`) and
/// forward them, as one or more `protocol_protocol::Event`s, only to the
/// sessions whose player is actually in an affected room - so a networked
/// client (a plain MUD client, or a future Godot client speaking this same
/// protocol) reacts only to what happened around it.
///
/// `game_world` must be the same `GameWorld` `events` was drained from -
/// callers pass in the read/write guard they already hold (it derefs to
/// `&GameWorld`) *before* dropping it, so the room lookups below see
/// up-to-date state. See the `MoveHandler`/`AttackHandler`/
/// `CreateCharacterHandler` call sites.
///
/// Commands are grouped by affected room and sent as one `Event` per
/// (room, recipient set) rather than one `Event` per command, to avoid a
/// flood of tiny messages. When a command's room can't be determined (see
/// `affected_room`), it's broadcast to every session instead of being
/// silently dropped - clients receiving an update for a room they're not in
/// can ignore it, which is safer than a player missing a message meant for
/// them.
fn dispatch_events(
    events: Vec<protocol_domain::DomainEvent>,
    session_manager: &SessionManager,
    game_world: &GameWorld,
) {
    if events.is_empty() {
        return;
    }

    let commands: Vec<protocol_presentation::PresentationCommand> = events
        .iter()
        .flat_map(protocol_presentation::translate_event)
        .collect();

    if commands.is_empty() {
        return;
    }

    // Group commands by the room they affect (`None` = undetermined, falls
    // back to a full broadcast) so we send one batch per recipient set
    // instead of one message per command.
    let mut by_room: std::collections::HashMap<
        Option<u32>,
        Vec<protocol_presentation::PresentationCommand>,
    > = std::collections::HashMap::new();
    for command in commands {
        let room_id = affected_room(&command, game_world);
        by_room.entry(room_id).or_default().push(command);
    }

    // Resolve each active session's current room once, up front, so room
    // groups below can just filter this list instead of re-querying
    // session_manager/game_world per group.
    let session_rooms: Vec<(u64, Option<u32>)> = session_manager
        .session_ids()
        .into_iter()
        .map(|session_id| {
            let room_id = session_manager
                .get_player_id(session_id)
                .and_then(|character_id| game_world.get_character(character_id))
                .map(|character| character.room_id);
            (session_id, room_id)
        })
        .collect();

    tracing::debug!(
        "dispatch_events: {} command(s) in {} room group(s); sessions: {:?}",
        by_room.values().map(|v| v.len()).sum::<usize>(),
        by_room.len(),
        session_rooms
    );

    for (room_id, batch) in by_room {
        let batch_len = batch.len();
        let payload = match serde_json::to_string(&batch) {
            Ok(json) => json.into_bytes(),
            Err(e) => {
                tracing::warn!("Failed to serialize presentation commands: {}", e);
                continue;
            }
        };

        let event = Event {
            id: rand::random(),
            event_type: "presentation_batch".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            source: "server".to_string(),
            payload,
            targets: None,
        };
        let message = Message::event(event);

        match room_id {
            None => {
                tracing::debug!(
                    "presentation_batch: could not determine an affected room for {} command(s); broadcasting to all sessions",
                    batch_len
                );
                session_manager.broadcast(&message, None);
            }
            Some(room_id) => {
                let mut sent = 0usize;
                for (session_id, session_room) in &session_rooms {
                    if *session_room == Some(room_id) {
                        match session_manager.send_to(*session_id, message.clone()) {
                            Ok(()) => sent += 1,
                            Err(e) => tracing::warn!(
                                "presentation_batch: failed to send to session {}: {}",
                                session_id,
                                e
                            ),
                        }
                    }
                }
                tracing::debug!(
                    "presentation_batch: {} command(s) for room {} -> {} session(s)",
                    batch_len,
                    room_id,
                    sent
                );
            }
        }
    }
}

/// Which room (if any) a `PresentationCommand` affects, for targeting
/// purposes. `EnterRoom`/`LeaveRoom`/`SpawnEntity` carry a `room_id`
/// directly; everything else that only carries an `entity_id` is resolved
/// via `entity_room`. `ShowMessage`'s `target_entity_id` is optional and
/// `PlayEffect`'s `entity_id` is optional too - both fall back to `None`
/// (broadcast) when absent, same as when the entity can't be found at all.
fn affected_room(
    command: &protocol_presentation::PresentationCommand,
    game_world: &GameWorld,
) -> Option<u32> {
    use protocol_presentation::PresentationCommand::*;
    match command {
        SpawnEntity { room_id, .. } => Some(*room_id),
        EnterRoom { room_id, .. } => Some(*room_id),
        LeaveRoom { room_id, .. } => Some(*room_id),
        DespawnEntity { entity_id } => entity_room(*entity_id, game_world),
        UpdateProperty { entity_id, .. } => entity_room(*entity_id, game_world),
        PlayEffect { entity_id, .. } => entity_id.and_then(|id| entity_room(id, game_world)),
        ShowMessage {
            target_entity_id, ..
        } => target_entity_id.and_then(|id| entity_room(id, game_world)),
    }
}

/// Resolve a presentation `entity_id` back to the room it's currently in.
/// `Character` and `Npc` share one `entity_id` space (see the id-space
/// comment on `application::GameWorld::new()`): NPCs are the small
/// statically-defined 1-4 range from `World::initialize()`, and character
/// ids start at 1000. This threshold is the same pragmatic reservation
/// that comment describes, not a real type tag - if NPCs ever get created
/// dynamically this needs a proper discriminated id instead.
fn entity_room(entity_id: u64, game_world: &GameWorld) -> Option<u32> {
    if entity_id >= 1000 {
        game_world.get_character(entity_id).map(|c| c.room_id)
    } else {
        game_world
            .get_world()
            .get_npc(entity_id)
            .map(|npc| npc.room_id)
    }
}

struct LookHandler {
    game_world: Arc<RwLock<GameWorld>>,
    session_manager: Arc<SessionManager>,
}

#[async_trait::async_trait]
impl protocol_routing::CommandHandler for LookHandler {
    async fn handle(
        &self,
        _command: Command,
        session_id: u64,
    ) -> Result<CommandResponse, protocol_routing::RoutingError> {
        let character_id = resolve_character_id(&self.session_manager, session_id)?;

        let world = self.game_world.read().await;
        let room_id = world
            .get_character(character_id)
            .ok_or_else(|| {
                protocol_routing::RoutingError::HandlerError("Character not found".to_string())
            })?
            .room_id;
        let room_info = world.look_room(room_id).ok_or_else(|| {
            protocol_routing::RoutingError::HandlerError("Room not found".to_string())
        })?;

        let response = LookResponse {
            room_name: room_info.name,
            room_description: room_info.description,
            exits: room_info.exits,
            players: room_info
                .players
                .into_iter()
                .map(|p| protocol_protocol::PlayerSummary {
                    id: p.id,
                    name: p.name,
                    level: p.level,
                })
                .collect(),
            npcs: room_info
                .npcs
                .into_iter()
                .map(|n| protocol_protocol::NpcSummary {
                    id: n.id,
                    name: n.name,
                    hp: n.hp,
                    max_hp: n.max_hp,
                })
                .collect(),
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
    async fn handle(
        &self,
        command: Command,
        session_id: u64,
    ) -> Result<CommandResponse, protocol_routing::RoutingError> {
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
        let move_result = world.move_character(character_id, domain_dir);
        let events = world.drain_events();
        // Look up affected rooms/sessions while still holding the write
        // guard (it derefs to `&GameWorld`) - dropping first would leave
        // dispatch_events with nothing to resolve entity/session rooms
        // against.
        dispatch_events(events, &self.session_manager, &world);
        drop(world);

        match move_result {
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
    async fn handle(
        &self,
        command: Command,
        session_id: u64,
    ) -> Result<CommandResponse, protocol_routing::RoutingError> {
        let character_id = resolve_character_id(&self.session_manager, session_id)?;
        let target_name = String::from_utf8_lossy(&command.payload).to_string();

        let mut world = self.game_world.write().await;
        let combat_result = world.start_combat(character_id, &target_name);
        let events = world.drain_events();
        // See MoveHandler above: look up rooms while still holding the
        // write guard, then drop.
        dispatch_events(events, &self.session_manager, &world);
        drop(world);

        match combat_result {
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
    async fn handle(
        &self,
        _command: Command,
        session_id: u64,
    ) -> Result<CommandResponse, protocol_routing::RoutingError> {
        let character_id = resolve_character_id(&self.session_manager, session_id)?;

        let world = self.game_world.read().await;
        let inventory = world
            .get_inventory(character_id)
            .cloned()
            .unwrap_or_else(Inventory::new);

        let response = InventoryResponse {
            items: inventory
                .items
                .into_iter()
                .map(|i| protocol_protocol::InventoryItem {
                    item_id: i.item_id,
                    name: i.name,
                    quantity: i.quantity,
                    item_type: "item".to_string(),
                })
                .collect(),
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
    async fn handle(
        &self,
        command: Command,
        session_id: u64,
    ) -> Result<CommandResponse, protocol_routing::RoutingError> {
        let create_cmd: CreateCharacterCommand = rmp_serde::from_slice(&command.payload)
            .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;

        let mut world = self.game_world.write().await;
        match world.create_character(create_cmd.name, &create_cmd.class) {
            Ok(character) => {
                let character_id = character.id;
                world.add_character(character);

                let events = world.drain_events();

                // Bind the session to its new character BEFORE dispatching.
                // Order matters now that dispatch is room-scoped: it
                // resolves each session's room via
                // session -> player_id -> Character.room_id, so an unbound
                // session has no room and matches nothing. Dispatching
                // first meant the player who just created a character was
                // the one client that never saw their own SpawnEntity.
                // (Found by the Godot network test, not by reasoning.)
                self.session_manager
                    .set_player(session_id, character_id)
                    .map_err(|e| protocol_routing::RoutingError::HandlerError(e.to_string()))?;

                // Still holding the write guard (it derefs to &GameWorld)
                // so the room lookups see current state; drop after.
                dispatch_events(events, &self.session_manager, &world);
                drop(world);

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
