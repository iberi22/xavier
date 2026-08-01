//! Agent memory management CLI handlers (client-side)

use anyhow::Result;
use colored::*;
use serde_json::json;

use crate::cli::commands::enums::{AgentCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{require_xavier_token, resolve_base_url};

/// Handle agent commands.
pub async fn handle_agent_command(cmd: AgentCommand) -> Result<()> {
    match cmd {
        AgentCommand::Scan { agent, json } => handle_agent_scan(agent, json).await,
        AgentCommand::Index { agent, json } => handle_agent_index(agent, json).await,
        AgentCommand::Push { agent, json } => handle_agent_sync(agent, false, json).await,
        AgentCommand::Pull { agent, json } => handle_agent_sync(agent, true, json).await,
    }
}

async fn handle_agent_scan(agent_filter: Option<String>, as_json: bool) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    if !as_json {
        println!("{} Scanning for agent sessions...", "🔍".cyan());
    }

    let resp = client
        .get(format!("{}/xavier/openclaw/scan", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let mut data: serde_json::Value = resp.json().await?;

        // Filter by agent name if provided
        if let Some(filter) = agent_filter {
            if let Some(agents) = data["agents"].as_array_mut() {
                agents.retain(|s| {
                    s["agent_id"]
                        .as_str()
                        .map(|name| name.to_lowercase().contains(&filter.to_lowercase()))
                        .unwrap_or(false)
                });
                data["count"] = json!(agents.len());
            }
        }

        if as_json {
            println!("{}", serde_json::to_string_pretty(&data)?);
        } else {
            let count = data["count"].as_u64().unwrap_or(0);
            println!("{} Found {} OpenClaw agents", "✅".green(), count);
            if let Some(agents) = data["agents"].as_array() {
                for s in agents {
                    let memory_md = s["memory_md"].as_str().is_some();
                    println!(
                        "  • {} (MEMORY.md: {})",
                        s["agent_id"].as_str().unwrap_or("Unknown").bold(),
                        if memory_md { "YES".green() } else { "NO".red() }
                    );
                }
            }
        }
    } else {
        let err = resp.text().await?;
        if as_json {
            println!("{}", json!({"status": "error", "message": err}));
        } else {
            println!("{} Scan failed: {}", "❌".red(), err);
        }
    }

    Ok(())
}

async fn handle_agent_index(_agent_filter: Option<String>, as_json: bool) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    if !as_json {
        println!("{} Indexing agent sessions into memory...", "🤖".cyan());
    }

    let resp = client
        .post(format!("{}/xavier/openclaw/index", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let data: serde_json::Value = resp.json().await?;
        if as_json {
            println!("{}", serde_json::to_string_pretty(&data)?);
        } else {
            let count = data["indexed_count"].as_u64().unwrap_or(0);
            println!("{} Successfully indexed {} sessions", "✅".green(), count);
        }
    } else {
        let err = resp.text().await?;
        if as_json {
            println!("{}", json!({"status": "error", "message": err}));
        } else {
            println!("{} Indexing failed: {}", "❌".red(), err);
        }
    }

    Ok(())
}

async fn handle_agent_sync(agent_filter: Option<String>, pull: bool, as_json: bool) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    let mode = if pull { "pull" } else { "push" };
    if !as_json {
        println!(
            "{} Syncing agent memory ({}) to cloud...",
            "🔄".cyan(),
            mode
        );
    }

    let resp = client
        .post(format!("{}/xavier/agents/sync", base_url))
        .header("X-Xavier-Token", &token)
        .json(&json!({ "mode": mode, "agent": agent_filter }))
        .send()
        .await?;

    if resp.status().is_success() {
        let data: serde_json::Value = resp.json().await?;
        if as_json {
            println!("{}", serde_json::to_string_pretty(&data)?);
        } else {
            println!("{} Agent memory synchronization completed", "✅".green());
            if let Some(stats) = data["stats"].as_object() {
                for (k, v) in stats {
                    println!("  • {}: {}", k, v);
                }
            }
        }
    } else {
        let err = resp.text().await?;
        if as_json {
            println!("{}", json!({"status": "error", "message": err}));
        } else {
            println!("{} Synchronization failed: {}", "❌".red(), err);
        }
    }

    Ok(())
}
