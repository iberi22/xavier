//! CLI command implementation sub-modules
//!
//! This module splits the former monolithic `commands.rs` into focused
//! sub-modules:
//!
//! - [`enums`] — Command, UsageCommand, CodeCommand, TokenCommand, SecretsCommand
//! - [`http`] — HTTP-based API calls (recall, stats, export, session_save)
//! - [`code`] — Code graph queries
//! - [`spawn`] — Agent spawning, multi-spawn, swarm
//! - [`token`] — Token generation
//! - [`usage`] — Provider usage tracking
//! - [`secrets`] — Ephemeral secret management (Clavis)
//!
//! The top-level [`Command`] enum and [`Cli::run()`] dispatch remain visible
//! through re-exports so that external consumers are unaffected.

pub mod enums;
pub mod code;
pub mod http;
pub mod secrets;
pub mod spawn;
pub mod token;
pub mod usage;

// Re-export the enums for backward compatibility
pub use enums::*;

use crate::cli::config::{resolve_base_url, require_xavier_token};
use crate::cli::security::secure_cli_input;
use crate::cli::server::{add_memory_hierarchical, search_memories_filtered, start_http_server};
use crate::cli::mcp::start_mcp_stdio;
use crate::cli::state::Cli;

use anyhow::Result;

use xavier::memory::qmd_memory::MemoryDocument;

impl Cli {
    /// Run the selected subcommand.
    pub async fn run(&self) -> Result<()> {
        use enums::Command;

        match self.cmd.as_ref().unwrap_or(&Command::Http { port: None }) {
            Command::Http { port } => {
                let port = port.unwrap_or_else(resolve_http_port);
                start_http_server(port).await
            }
            Command::Mcp => start_mcp_stdio().await,
            Command::Search {
                query,
                limit,
                cluster,
                level,
            } => {
                let base_url = resolve_base_url();
                println!("Searching memories via HTTP API on {}", base_url);
                let lim = limit.unwrap_or(10);
                search_memories_filtered(query, lim, cluster.clone(), level.clone()).await
            }
            Command::Usage { cmd } => usage::handle_usage_command(cmd.clone()).await,
            Command::Add {
                content,
                title,
                kind,
                cluster,
                level,
                relation,
            } => {
                println!("Adding memory...");
                add_memory_hierarchical(
                    content,
                    title.as_ref().map(|s| s.as_str()),
                    kind.as_deref(),
                    cluster.as_deref(),
                    level.as_deref(),
                    relation.as_deref(),
                )
                .await
            }
            Command::Recall { query, limit } => http::recall_memories(query, *limit).await,
            Command::ExportPack {
                topic,
                max_level,
                out,
            } => http::export_context_pack(topic, *max_level, out).await,
            Command::Stats => {
                println!("Fetching Xavier statistics...");
                http::show_stats().await
            }
            Command::Code { cmd } => code::handle_code_command(cmd.clone()).await,
            Command::SessionSave {
                session_id,
                content,
            } => http::session_save(session_id, content).await,
            Command::Spawn {
                count,
                provider,
                model,
                skills,
                context,
                task,
            } => {
                spawn::spawn_agents(
                    *count,
                    provider.clone(),
                    model.clone(),
                    skills,
                    context,
                    task.as_deref(),
                )
                .await
            }
            Command::MultiSpawn {
                agents,
                batch,
                provider,
                model,
                skills,
                task,
            } => {
                spawn::multi_spawn_agents(
                    *agents,
                    *batch,
                    provider.clone(),
                    model.clone(),
                    skills.clone(),
                    task.as_deref(),
                )
                .await
            }
            Command::Swarm { config, parallel } => {
                spawn::run_swarm(config.clone(), *parallel).await
            }
            Command::Chronicle { cmd } => {
                xavier::chronicle::cli::handle_chronicle_command(cmd.clone()).await
            }
            Command::Token { cmd } => token::handle_token_command(cmd.clone()).await,
            Command::Secrets { cmd } => secrets::handle_secrets_command(cmd.clone()).await,
            Command::Export { public, output } => {
                let base_url = resolve_base_url();
                let token = require_xavier_token()?;
                let client = enums::CLI_HTTP_CLIENT.clone();

                println!("Exporting memories (public_only={})...", public);
                let resp = client
                    .get(format!("{}/memory/export?public={}", base_url, public))
                    .header("X-Xavier-Token", &token)
                    .send()
                    .await?;

                if resp.status().is_success() {
                    let docs: Vec<MemoryDocument> = resp.json().await?;
                    let json = serde_json::to_string_pretty(&docs)?;

                    if let Some(path) = output {
                        std::fs::write(path, json)?;
                        println!("✅ Exported {} memories to {}", docs.len(), path.display());
                    } else {
                        println!("{}", json);
                        println!("\n✅ Exported {} memories to stdout", docs.len());
                    }
                } else {
                    println!("❌ Export failed: {}", resp.text().await?);
                }
                Ok(())
            }
        }
    }
}
