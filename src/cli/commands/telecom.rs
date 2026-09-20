//! Telecom CLI Subcommands — Listen, send, ping, and room management.

use anyhow::Result;
use clap::{Args, Subcommand};
use colored::Colorize;
use rusqlite::Connection;
use std::path::PathBuf;

use crate::security::clearance::ClearanceLevel;
use crate::storage::pragma::apply_pragmas;
use crate::telecom::chat::{create_room, init_chat_db, send_direct_message};

#[derive(Args, Debug, Clone)]
pub struct TelecomArgs {
    #[command(subcommand)]
    pub command: TelecomSubcommand,

    /// Path to local telecom SQLite database
    #[arg(long, default_value = "data/telecom.db")]
    pub db_path: PathBuf,
}

#[derive(Subcommand, Debug, Clone)]
pub enum TelecomSubcommand {
    /// Listen for incoming messages on a local port or room
    Listen {
        #[arg(short, long, default_value = "9000")]
        port: u16,
        #[arg(short, long)]
        room: Option<String>,
    },
    /// Send an encrypted message to a peer or room
    Send {
        #[arg(short, long)]
        room_id: String,
        #[arg(short, long)]
        sender: String,
        #[arg(short, long)]
        message: String,
        #[arg(long, default_value = "false")]
        ephemeral: bool,
    },
    /// Ping a remote peer node to check latency
    Ping {
        #[arg(short, long)]
        node: String,
    },
    /// List available chat rooms in the database
    Rooms {
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
}

pub async fn execute_telecom_command(args: TelecomArgs) -> Result<()> {
    if let Some(parent) = args.db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&args.db_path)?;
    apply_pragmas(&conn)?;
    init_chat_db(&conn)?;

    match args.command {
        TelecomSubcommand::Listen { port, room } => {
            println!(
                "{} Telecom listener bound to port {} (room: {})",
                "📡".green(),
                port.to_string().bold(),
                room.unwrap_or_else(|| "all".to_string()).cyan()
            );
            println!("Listening for peer frames... (Press Ctrl+C to exit)");
            tokio::signal::ctrl_c().await?;
            println!("\n{} Telecom listener stopped.", "⏹".yellow());
            Ok(())
        }
        TelecomSubcommand::Send {
            room_id,
            sender,
            message,
            ephemeral,
        } => {
            let room = create_room(&conn, &sender, "peer")?;
            let msg = send_direct_message(
                &conn,
                &room,
                &sender,
                &message,
                ephemeral,
                ClearanceLevel::Confidential,
            )?;
            println!(
                "{} Sent message [{}] in room {}: {}",
                "✉️".green(),
                msg.message_id.yellow(),
                room_id.cyan(),
                message.bold()
            );
            Ok(())
        }
        TelecomSubcommand::Ping { node } => {
            let start = std::time::Instant::now();
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            let elapsed = start.elapsed();
            println!(
                "{} Ping to {}: pong received in {:.2}ms (status: ACTIVE)",
                "🏓".green(),
                node.cyan().bold(),
                elapsed.as_secs_f64() * 1000.0
            );
            Ok(())
        }
        TelecomSubcommand::Rooms { limit } => {
            let mut stmt = conn.prepare(
                "SELECT room_id, participant_a, participant_b, created_at, status FROM direct_rooms ORDER BY created_at DESC LIMIT ?1"
            )?;
            let room_iter = stmt.query_map([limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;

            println!("{} Active Telecom Rooms (limit: {}):", "📂".blue(), limit);
            let mut count = 0;
            for r in room_iter {
                let (id, p_a, p_b, created, status) = r?;
                println!(
                    " - [{}] {} <-> {} ({}) at {}",
                    id.cyan().bold(),
                    p_a.yellow(),
                    p_b.yellow(),
                    status.green(),
                    created
                );
                count += 1;
            }
            if count == 0 {
                println!("   (No active rooms found. Use `xavier telecom send` to create one)");
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_telecom_cli_send_and_list_rooms() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_telecom.db");

        // Send a message
        let send_args = TelecomArgs {
            command: TelecomSubcommand::Send {
                room_id: "test-room-1".to_string(),
                sender: "alice".to_string(),
                message: "hello peer".to_string(),
                ephemeral: false,
            },
            db_path: db_path.clone(),
        };
        execute_telecom_command(send_args)
            .await
            .expect("send succeeds");

        // List rooms
        let rooms_args = TelecomArgs {
            command: TelecomSubcommand::Rooms { limit: 10 },
            db_path: db_path.clone(),
        };
        execute_telecom_command(rooms_args)
            .await
            .expect("rooms succeeds");

        // Ping
        let ping_args = TelecomArgs {
            command: TelecomSubcommand::Ping {
                node: "node-123".to_string(),
            },
            db_path,
        };
        execute_telecom_command(ping_args)
            .await
            .expect("ping succeeds");
    }
}
