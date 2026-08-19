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

pub mod billing;
pub mod cleanup;
pub mod code;
pub mod data_commons;
pub mod enums;
pub mod governance;
pub mod http;
pub mod improve;
pub mod license;
pub mod memory;
pub mod mesh;
pub mod navigation;
pub mod node;
pub mod nodes;
pub mod provider;
pub mod regen;
pub mod secrets;
pub mod session;
pub mod spawn;
pub mod tasks;
pub mod token;
pub mod usage;
pub mod verify;
pub mod wallet;

// Re-export for backward compatibility
pub use enums::*;
#[allow(unused_imports)]
pub use spawn::load_spawn_memory;

use crate::cli::config::{require_xavier_token, resolve_base_url, resolve_http_port};
use crate::cli::mcp::start_mcp_stdio;
use crate::cli::server::{add_memory_hierarchical, search_memories_filtered, start_http_server};
use crate::cli::state::Cli;

use anyhow::Result;

use xavier::memory::qmd_memory::MemoryDocument;

impl Cli {
    /// Run the selected subcommand.
    pub async fn run(&self) -> Result<()> {
        use enums::Command;

        match self.cmd.as_ref().unwrap_or(&Command::Http {
            port: None,
            mcp_port: None,
        }) {
            Command::Http { port, mcp_port } => {
                let port = port.unwrap_or_else(resolve_http_port);
                start_http_server(port, *mcp_port).await
            }
            Command::Mcp => start_mcp_stdio().await,
            Command::IndexSelf => {
                println!("IndexSelf is not yet implemented.");
                Ok(())
            }
            Command::Search {
                query,
                limit,
                max_results,
                cluster,
                level,
                offline_ok,
            } => {
                let base_url = resolve_base_url();
                println!("Searching memories via HTTP API on {}", base_url);
                // Prefer --max-results / -n flag over positional limit
                let lim = (*max_results).or(*limit).unwrap_or(10);
                search_memories_filtered(query, lim, cluster.clone(), level.clone(), *offline_ok)
                    .await
            }
            Command::Chat {
                prompt,
                agent,
                interactive,
                json,
                limit,
                model,
            } => {
                crate::cli::handlers::chat::handle_chat_command(
                    prompt.clone(),
                    agent.clone(),
                    *interactive,
                    *json,
                    *limit,
                    model.clone(),
                )
                .await
            }
            Command::Ask {
                prompt,
                agent,
                json,
                limit,
                model,
            } => {
                crate::cli::handlers::chat::handle_chat_command(
                    Some(prompt.clone()),
                    agent.clone(),
                    false,
                    *json,
                    *limit,
                    model.clone(),
                )
                .await
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
            Command::Recall {
                query,
                limit,
                offline_ok,
            } => http::recall_memories(query, *limit, *offline_ok).await,
            Command::ExportPack {
                topic,
                max_level,
                out,
            } => http::export_context_pack(topic, *max_level, out).await,
            Command::Stats { offline_ok } => {
                println!("Fetching Xavier statistics...");
                http::show_stats(*offline_ok).await
            }
            Command::Reindex => {
                println!("Re-indexing memories missing embeddings...");
                http::reindex_memories().await
            }
            Command::Code { cmd } => code::handle_code_command(cmd.clone()).await,
            Command::Ls { path } => navigation::handle_ls(path.clone()).await,
            Command::Cd { path } => navigation::handle_cd(path.clone()).await,
            Command::Pwd => navigation::handle_pwd().await,
            Command::Nav { cmd } => match cmd {
                NavCommand::Ls { path } => navigation::handle_ls(path.clone()).await,
                NavCommand::Cd { path } => navigation::handle_cd(path.clone()).await,
                NavCommand::Pwd => navigation::handle_pwd().await,
                NavCommand::Affected {
                    path,
                    depth,
                    format,
                    exclude_file_type,
                } => {
                    navigation::handle_affected(
                        path.clone(),
                        *depth,
                        format.clone(),
                        exclude_file_type.clone(),
                    )
                    .await
                }
                NavCommand::Visualize {
                    format,
                    hotspots,
                    tree,
                    output,
                } => {
                    navigation::handle_visualize(format.clone(), *hotspots, *tree, output.clone())
                        .await
                }
                NavCommand::Telemetry { kind } => navigation::handle_telemetry(kind.clone()).await,
            },
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
            Command::Provider { cmd } => provider::handle_provider_command(cmd.clone()).await,
            Command::Setup { local } => crate::cli::handlers::setup::handle_setup(*local).await,
            Command::Doctor { format, verbose } => {
                crate::cli::handlers::doctor::handle_doctor(format.clone(), *verbose).await
            }
            Command::DataCommons { cmd } => {
                data_commons::handle_data_commons_command(cmd.clone()).await
            }
            Command::Governance { command } => governance::handle_governance(command.clone()).await,
            Command::Wallet { cmd } => wallet::handle_wallet_command(cmd.clone()).await,
            Command::Session { cmd } => session::handle_session_command(cmd.clone()).await,
            Command::Mesh { cmd } => mesh::handle_mesh_command(cmd.clone()).await,
            Command::Node { cmd } => node::handle_node_command(cmd.clone()).await,
            Command::Nodes { cmd } => nodes::handle_nodes_command(cmd.clone()).await,
            Command::Secrets { cmd } => secrets::handle_secrets_command(cmd.clone()).await,
            Command::Vault { cmd } => secrets::handle_vault_command(cmd.clone()).await,
            Command::Quota => crate::cli::handlers::quota::handle_quota_command().await,
            Command::Tasks { cmd } => tasks::handle_tasks_command(cmd.clone()).await,
            Command::Billing => crate::cli::handlers::billing::handle_billing_command().await,
            Command::Task { cmd } => {
                crate::cli::handlers::tasks::handle_task_command(cmd.clone()).await
            }
            Command::Sync { cmd: _ } => crate::cli::handlers::sync::handle_sync_command().await,
            Command::Verify { cmd } => verify::handle_verify_command(cmd.clone()).await,
            Command::Cloud { cmd } => {
                crate::cli::handlers::cloud::handle_cloud_command(cmd.clone()).await
            }
            Command::Agent { cmd } => {
                crate::cli::handlers::agent_cli::handle_agent_command(cmd.clone()).await
            }
            Command::Plugin { cmd } => match cmd {
                PluginCommand::Install { name } => {
                    crate::cli::handlers::plugins::install_plugin(name.clone()).await
                }
                PluginCommand::List => crate::cli::handlers::plugins::list_plugins().await,
            },
            Command::Scan { cmd } => {
                crate::cli::handlers::system_scan_cli::handle_scan_command(cmd.clone()).await
            }
            Command::License { cmd } => {
                crate::cli::commands::license::handle_license_command(cmd.clone()).await
            }
            Command::Memory { cmd } => memory::handle_memory_command(cmd.clone()).await,
            Command::Export {
                public,
                output,
                limit,
            } => {
                let base_url = resolve_base_url();
                let token = require_xavier_token()?;
                let client = enums::CLI_HTTP_CLIENT.clone();

                let limit = limit.unwrap_or(1000).clamp(1, 10000);
                println!(
                    "Exporting memories (public_only={}, limit={})...",
                    public, limit
                );
                let resp = client
                    .get(format!(
                        "{}/memory/export?public={}&limit={}",
                        base_url, public, limit
                    ))
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
            Command::Maturity { cmd } => {
                xavier::maturity::cli::handle_maturity_command(cmd.clone()).await
            }
            Command::Health { cloud } => {
                crate::cli::handlers::system::handle_health_command(*cloud).await
            }
            Command::Improve { ci, cmd } => improve::handle_improve_command(*ci, cmd.clone()).await,
            Command::Regen { cmd } => regen::handle_regen_command(cmd.clone()).await,
            Command::Cleanup {
                dry_run,
                apply,
                days,
            } => cleanup::handle_cleanup(*dry_run, *apply, *days).await,
        }
    }
}
