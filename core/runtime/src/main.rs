use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
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

/// Client mode.
///
/// The interactive REPL lives in the `mud` binary (`clients/mud`); both it
/// and this share `protocol-client` for the wire protocol. This subcommand
/// stays because `runtime client` is a documented entry point, but it does
/// not duplicate the REPL - it runs a short scripted session, which is also
/// what makes it usable as a smoke check against a running server.
async fn run_client(server_addr: &str) -> Result<()> {
    use protocol_client::{args, describe, Connection, Pushed};

    tracing::info!("Connecting to {}", server_addr);
    let mut conn = Connection::connect(server_addr).await?;
    println!("Connected to server. Session: {}", conn.session_id());

    let steps: Vec<(&str, Vec<u8>)> = vec![
        (
            "create_character",
            args::create_character("Runtime Client", "warrior")?,
        ),
        ("look", args::none()),
        ("inventory", args::none()),
    ];

    for (command, payload) in steps {
        let response = conn.request(command, payload).await?;
        println!(
            "{}: {}",
            command,
            if response.success {
                "ok".to_string()
            } else {
                response.error.clone().unwrap_or_default()
            }
        );

        for pushed in conn.take_pushed() {
            if let Pushed::Presentation(commands) = pushed {
                for command in &commands {
                    println!("  {}", describe(command));
                }
            }
        }
    }

    println!("Done. For an interactive session, run the `mud` binary.");
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
