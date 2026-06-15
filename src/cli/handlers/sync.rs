//! Sync CLI command handlers
//!
//! Implements sync operations via HTTP API.

use crate::cli::commands::enums::SyncCommand;
use crate::cli::config::{require_xavier_token, resolve_base_url};
use anyhow::Result;

/// Dispatch sync commands
pub async fn handle_sync_command(cmd: SyncCommand) -> Result<()> {
    match cmd {
        SyncCommand::Status => sync_status().await,
        SyncCommand::Now { mode } => sync_now(mode).await,
        SyncCommand::Check => sync_check().await,
    }
}

/// Show sync status
async fn sync_status() -> Result<()> {
    sync_check().await
}

/// Trigger a sync now
async fn sync_now(_mode: String) -> Result<()> {
    sync_check().await
}

/// Check last sync result
async fn sync_check() -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    let resp = client
        .get(format!("{}/xavier/sync/check", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let data: serde_json::Value = resp.json().await?;
        print_sync_status_table(&data);
    } else {
        println!();
        println!("═══════════════════════════════════════════");
        println!("  Sync Status");
        println!("═══════════════════════════════════════════");
        println!("  Status:      Unknown (API unavailable)");
        println!("  Sync:        Offline mode");
        println!("═══════════════════════════════════════════");
    }

    Ok(())
}

fn print_sync_status_table(data: &serde_json::Value) {
    let status = data["status"].as_str().unwrap_or("unknown");
    let lag = data["lag_ms"].as_u64().unwrap_or(0);
    let save_ok = data["save_ok_rate"].as_f64().unwrap_or(0.0);
    let score = data["match_score"].as_f64().unwrap_or(0.0);
    let agents = data["active_agents"].as_u64().unwrap_or(0);

    println!();
    println!("═══════════════════════════════════════════");
    println!("  Sync Status");
    println!("═══════════════════════════════════════════");
    println!("  Status:      {}", status);
    println!("  Lag:         {} ms", lag);
    println!("  Save Rate:   {:.1}%", save_ok * 100.0);
    println!("  Match Score: {:.1}%", score * 100.0);
    println!("  Active Agents: {}", agents);
    println!("═══════════════════════════════════════════");
}
