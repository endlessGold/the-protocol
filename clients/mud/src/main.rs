use anyhow::Result;
use bytes::{BufMut, BytesMut};
use clap::Parser;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

use protocol_observability;
use protocol_protocol::*;

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
    tracing::info!("Connecting to {}...", cli.server);

    use tokio::net::TcpStream;
    let mut stream = TcpStream::connect(&cli.server).await?;
    stream.set_nodelay(true)?;

    let (mut reader, mut writer) = stream.into_split();
    let codec = ProtocolCodec::new();

    // Handshake
    let hello = Message::hello(ClientType::MUD, None);
    writer.write_all(&codec.encode(&hello)?).await?;

    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let total_len = u32::from_be_bytes(len_buf) as usize;
    let mut frame = vec![0u8; total_len - 4];
    reader.read_exact(&mut frame).await?;

    let mut full_frame = BytesMut::with_capacity(4 + total_len);
    full_frame.put_slice(&len_buf);
    full_frame.put_slice(&frame);

    let mut buf = full_frame;
    let ack =
        ProtocolCodec::decode_simple(&mut buf)?.ok_or_else(|| anyhow::anyhow!("No response"))?;

    match ack.message_type {
        MessageType::HelloAck => {
            let hello_ack: HelloAck = rmp_serde::from_slice(&ack.payload)?;
            println!("Connected! Session: {}", hello_ack.session_id);
        }
        _ => return Err(anyhow::anyhow!("Handshake failed")),
    }

    println!("Type 'help' for commands.\n");

    let stdin = tokio::io::stdin();
    let mut input_reader = tokio::io::BufReader::new(stdin);
    let mut input = String::new();

    loop {
        input.clear();
        print!("> ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let bytes_read = input_reader.read_line(&mut input).await?;
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
                println!("Commands: look, move <dir>, attack <target>, inventory, create <name> <class>, quit");
                continue;
            }
            "look" => Message::command(Command {
                id: rand::random(),
                command_type: "look".to_string(),
                session_id: 0,
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                payload: vec![],
            }),
            "move" => {
                let dir = parts.get(1).unwrap_or(&"north");
                let move_cmd = MoveCommand {
                    direction: Direction::from_str(dir).unwrap_or(Direction::North),
                };
                Message::command(Command {
                    id: rand::random(),
                    command_type: "move".to_string(),
                    session_id: 0,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    payload: rmp_serde::to_vec(&move_cmd)?,
                })
            }
            "attack" => {
                let target = parts.get(1).unwrap_or(&"goblin");
                Message::command(Command {
                    id: rand::random(),
                    command_type: "attack".to_string(),
                    session_id: 0,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    payload: target.as_bytes().to_vec(),
                })
            }
            "inventory" => Message::command(Command {
                id: rand::random(),
                command_type: "inventory".to_string(),
                session_id: 0,
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                payload: vec![],
            }),
            "create" => {
                let name = parts.get(1).unwrap_or(&"Hero").to_string();
                let class = parts.get(2).unwrap_or(&"warrior").to_string();
                let create_cmd = CreateCharacterCommand { name, class };
                Message::command(Command {
                    id: rand::random(),
                    command_type: "create_character".to_string(),
                    session_id: 0,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    payload: rmp_serde::to_vec(&create_cmd)?,
                })
            }
            _ => {
                println!("Unknown command.");
                continue;
            }
        };

        writer.write_all(&codec.encode(&message)?).await?;

        match reader.read_exact(&mut len_buf).await {
            Ok(_n) => {
                let total_len = u32::from_be_bytes(len_buf) as usize;
                let mut frame = vec![0u8; total_len - 4];
                reader.read_exact(&mut frame).await?;

                let mut full_frame = BytesMut::with_capacity(4 + total_len);
                full_frame.put_slice(&len_buf);
                full_frame.put_slice(&frame);

                let mut buf = full_frame;
                if let Some(response) = ProtocolCodec::decode_simple(&mut buf)? {
                    match response.message_type {
                        MessageType::CommandResponse => {
                            let resp: CommandResponse = rmp_serde::from_slice(&response.payload)?;
                            if resp.success {
                                if let Ok(look) =
                                    rmp_serde::from_slice::<LookResponse>(&resp.payload)
                                {
                                    println!("\n=== {} ===", look.room_name);
                                    println!("{}", look.room_description);
                                    if !look.exits.is_empty() {
                                        println!("\nExits: {}", look.exits.join(", "));
                                    }
                                    for p in &look.players {
                                        println!("{} is here.", p.name);
                                    }
                                    for n in &look.npcs {
                                        println!("{} (HP: {}/{})", n.name, n.hp, n.max_hp);
                                    }
                                } else if let Ok(mv) =
                                    rmp_serde::from_slice::<MoveResponse>(&resp.payload)
                                {
                                    println!("Moved to {}.", mv.room_name.unwrap_or_default());
                                    println!("{}", mv.room_description.unwrap_or_default());
                                } else if let Ok(atk) =
                                    rmp_serde::from_slice::<AttackResponse>(&resp.payload)
                                {
                                    println!("{}", atk.message.unwrap_or_default());
                                } else if let Ok(inv) =
                                    rmp_serde::from_slice::<InventoryResponse>(&resp.payload)
                                {
                                    println!("\n=== Inventory ===");
                                    for item in &inv.items {
                                        println!("  {} x{}", item.name, item.quantity);
                                    }
                                    println!("Gold: {}", inv.gold);
                                } else {
                                    println!("Response received.");
                                }
                            } else {
                                println!("Error: {}", resp.error.unwrap_or_default());
                            }
                        }
                        MessageType::Error => {
                            let err: ErrorResponse = rmp_serde::from_slice(&response.payload)?;
                            println!("Error: {}", err.message);
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
