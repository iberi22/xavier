//! Sync CLI command handlers
//!
//! `xavier sync status|check` show the agent session-sync status of the local
//! server. `xavier sync now` runs the encrypted Xavier Cloud backup when
//! PGHEART_* is configured, and says plainly when nothing was uploaded.

use crate::cli::commands::enums::SyncCommand;
use crate::cli::config::{require_xavier_token, resolve_base_url};
use anyhow::Result;

/// Handle `xavier sync <cmd>`.
pub async fn handle_sync_command(cmd: SyncCommand) -> Result<()> {
    match cmd {
        SyncCommand::Now { mode } => handle_sync_now(&mode).await,
        SyncCommand::Status | SyncCommand::Check => handle_session_sync_status().await,
    }
}

async fn handle_sync_now(mode: &str) -> Result<()> {
    if crate::sync::xavier_cloud::CloudBackupConfig::from_env().is_none() {
        println!();
        println!(
            "No cloud backup is configured (PGHEART_URL / PGHEART_TOKEN): nothing was uploaded."
        );
        println!("Mesh peers sync in the background; `xavier mesh sync <node>` forces one.");
        return handle_session_sync_status().await;
    }
    if mode == "pull" {
        println!("Cloud backups are restored explicitly: run `xavier cloud restore`.");
        return Ok(());
    }
    println!("Running the encrypted Xavier Cloud backup (push)...");
    let report = crate::cli::handlers::cloud::run_cli_backup(None, false).await?;
    crate::cli::handlers::cloud::print_backup_report(&report, false)
}

/// Show the agent session-sync status (not a cloud backup).
async fn handle_session_sync_status() -> Result<()> {
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
        println!("  Agent Session Sync Status");
        println!("═══════════════════════════════════════════");
        println!("  Status:      ⚠️  Unknown (API unavailable)");
        println!("  Sync:        Offline mode");
        println!("═══════════════════════════════════════════");
    }
    let cloud = if crate::sync::xavier_cloud::CloudBackupConfig::from_env().is_some() {
        "configured (check it with `xavier cloud verify`)"
    } else {
        "not configured"
    };
    println!("  Cloud backup: {}", cloud);

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
    println!("  Agent Session Sync Status (not a cloud backup)");
    println!("═══════════════════════════════════════════");
    println!("  Status:      {}", status);
    println!("  Lag:         {} ms", lag);
    println!("  Save Rate:   {:.1}%", save_ok * 100.0);
    println!("  Match Score: {:.1}%", score * 100.0);
    println!("  Active Agents: {}", agents);
    println!("═══════════════════════════════════════════");
}
