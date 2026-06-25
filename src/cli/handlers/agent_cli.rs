//! Agent memory management CLI handlers (client-side)

use anyhow::Result;
use colored::*;
use serde_json::json;

use crate::cli::config::{require_xavier_token, resolve_base_url};
use crate::cli::commands::enums::{AgentCommand, CLI_HTTP_CLIENT};

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
        .get(format!("{}/xavier/agents/scan", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let mut data: serde_json::Value = resp.json().await?;

        // Filter by agent name if provided
        if let Some(filter) = agent_filter {
            if let Some(sessions) = data["sessions"].as_array_mut() {
                sessions.retain(|s| {
                    s["ide"].as_str().map(|name| name.to_lowercase().contains(&filter.to_lowercase())).unwrap_or(false)
                });
                data["count"] = json!(sessions.len());
            }
        }

        if as_json {
            println!("{}", serde_json::to_string_pretty(&data)?);
        } else {
            let count = data["count"].as_u64().unwrap_or(0);
            println!("{} Found {} agent sessions", "✅".green(), count);
            if let Some(sessions) = data["sessions"].as_array() {
                for s in sessions {
                    println!("  • {} (Source: {})",
                        s["ide"].as_str().unwrap_or("Unknown").bold(),
                        s["source_file"].as_str().unwrap_or("?")
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

async fn handle_agent_index(agent_filter: Option<String>, as_json: bool) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    if !as_json {
        println!("{} Indexing agent sessions into memory...", "🤖".cyan());
    }

    let resp = client
        .post(format!("{}/xavier/agents/index", base_url))
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
        println!("{} Syncing agent memory ({}) to cloud...", "🔄".cyan(), mode);
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
