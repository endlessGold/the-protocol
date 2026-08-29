//! Terminal MUD client.
//!
//! All the protocol work lives in `protocol-client`; this is just a REPL.
//! `core/runtime`'s `run_client()` is the same program behind a subcommand
//! and shares the same library.

use anyhow::Result;
use clap::Parser;
use protocol_client::{args, describe, Connection, Pushed};
use protocol_protocol::{
    AttackResponse, CommandResponse, Direction, InventoryResponse, LookResponse, MoveResponse,
};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Parser)]
#[command(name = "mud", about = "The Protocol - MUD Client")]
struct Cli {
    #[arg(short, long, default_value = "127.0.0.1:7770")]
    server: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    protocol_observability::init_logging();
    let cli = Cli::parse();
    run(&cli.server).await
}

async fn run(server: &str) -> Result<()> {
    tracing::info!("Connecting to {}...", server);
    let mut conn = Connection::connect(server).await?;
    println!("Connected. Session: {}", conn.session_id());
    println!("Type 'help' for available commands.");

    let mut lines = BufReader::new(tokio::io::stdin()).lines();

    loop {
        print!("> ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let Some(line) = lines.next_line().await? else {
            break; // EOF
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.splitn(2, ' ');
        let verb = parts.next().unwrap_or("").to_lowercase();
        let rest = parts.next().unwrap_or("").trim();

        let (command, payload) = match verb.as_str() {
            "quit" | "exit" => {
                println!("Goodbye!");
                break;
            }
            "help" => {
                print_help();
                continue;
            }
            "look" | "l" => ("look", args::none()),
            "inventory" | "inv" | "i" => ("inventory", args::none()),
            "move" | "go" | "m" => {
                let Some(direction) = Direction::from_str(rest) else {
                    println!("Go where? (north/south/east/west/up/down)");
                    continue;
                };
                ("move", args::movement(direction)?)
            }
            "attack" | "a" => {
                if rest.is_empty() {
                    println!("Attack what?");
                    continue;
                }
                ("attack", args::attack(rest))
            }
            "create" => {
                let mut it = rest.split_whitespace();
                let name = it.next().unwrap_or("Hero");
                let class = it.next().unwrap_or("warrior");
                ("create_character", args::create_character(name, class)?)
            }
            other => {
                println!("Unknown command '{}'. Try 'help'.", other);
                continue;
            }
        };

        match conn.request(command, payload).await {
            Ok(response) => print_response(command, &response),
            Err(e) => {
                println!("Error: {}", e);
                break;
            }
        }

        // Anything the server pushed while we waited. Before the shared
        // client existed these were silently swallowed as if they were the
        // command's reply.
        for pushed in conn.take_pushed() {
            match pushed {
                Pushed::Presentation(commands) => {
                    for command in &commands {
                        println!("  {}", describe(command));
                    }
                }
                Pushed::OtherEvent { event_type } => {
                    tracing::debug!("unhandled event type: {}", event_type);
                }
                Pushed::Disconnect => println!("  (server closed the session)"),
            }
        }
    }

    Ok(())
}

fn print_help() {
    println!("Available commands:");
    println!("  look              - Look around");
    println!("  move <direction>  - north/south/east/west/up/down");
    println!("  attack <target>   - Attack an NPC");
    println!("  inventory         - Check inventory");
    println!("  create <name> <class> - Create character (warrior/mage/rogue/cleric)");
    println!("  quit              - Disconnect");
}

fn print_response(command: &str, response: &CommandResponse) {
    if !response.success {
        println!(
            "Error: {}",
            response.error.as_deref().unwrap_or("unknown error")
        );
        return;
    }

    // Decode by the command we sent rather than guessing at the payload.
    // The old clients tried each response type in turn until one
    // deserialized, which quietly mis-rendered whenever two shapes happened
    // to be compatible.
    match command {
        "look" => match rmp_serde::from_slice::<LookResponse>(&response.payload) {
            Ok(look) => print_look(&look),
            Err(e) => println!("(couldn't read look response: {})", e),
        },
        "move" => match rmp_serde::from_slice::<MoveResponse>(&response.payload) {
            Ok(m) => {
                println!("\nYou move to {}.", m.room_name.unwrap_or_default());
                println!("{}", m.room_description.unwrap_or_default());
            }
            Err(e) => println!("(couldn't read move response: {})", e),
        },
        "attack" => match rmp_serde::from_slice::<AttackResponse>(&response.payload) {
            Ok(a) => println!("{}", a.message.unwrap_or_default()),
            Err(e) => println!("(couldn't read attack response: {})", e),
        },
        "inventory" => match rmp_serde::from_slice::<InventoryResponse>(&response.payload) {
            Ok(inv) => {
                println!("\n=== Inventory ===");
                if inv.items.is_empty() {
                    println!("  Empty");
                }
                for item in &inv.items {
                    println!("  {} x{}", item.name, item.quantity);
                }
                println!("Gold: {}", inv.gold);
            }
            Err(e) => println!("(couldn't read inventory response: {})", e),
        },
        _ => println!("OK"),
    }
}

fn print_look(look: &LookResponse) {
    println!("\n=== {} ===", look.room_name);
    println!("{}", look.room_description);
    if !look.exits.is_empty() {
        println!("\nExits: {}", look.exits.join(", "));
    }
    if !look.players.is_empty() {
        println!("\nPlayers here:");
        for p in &look.players {
            println!("  {} (Level {})", p.name, p.level);
        }
    }
    if !look.npcs.is_empty() {
        println!("\nNPCs here:");
        for n in &look.npcs {
            println!("  {} (HP: {}/{})", n.name, n.hp, n.max_hp);
        }
    }
}
