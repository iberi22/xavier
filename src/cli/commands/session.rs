//! CLI handlers for session management commands

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::cli::commands::enums::SessionCommand;
use crate::cli::config::{require_xavier_token, resolve_base_url};
use xavier::session::sharing::SessionBundle;

/// Handle session management commands
pub async fn handle_session_command(cmd: SessionCommand) -> Result<()> {
    match cmd {
        SessionCommand::Export { session_id, output } => export_session(session_id, output).await,
        SessionCommand::Import { input } => import_session(input).await,
        SessionCommand::Share { session_id, peer } => share_session(session_id, peer).await,
    }
}

async fn export_session(session_id: String, output: Option<PathBuf>) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    println!("Exporting session {}...", session_id);
    let resp = client
        .get(format!("{}/v1/sessions/{}/export", base_url, session_id))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let bundle: SessionBundle = resp.json().await?;
        let json = serde_json::to_string_pretty(&bundle)?;

        if let Some(path) = output {
            std::fs::write(&path, json)?;
            println!("✅ Exported session {} to {}", session_id, path.display());
        } else {
            println!("{}", json);
            println!("\n✅ Exported session {} to stdout", session_id);
        }
    } else {
        println!("❌ Export failed: {}", resp.text().await?);
    }

    Ok(())
}

async fn import_session(input: PathBuf) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    let raw = std::fs::read_to_string(&input).context("Failed to read session bundle file")?;
    let bundle: SessionBundle =
        serde_json::from_str(&raw).context("Failed to parse session bundle JSON")?;

    println!("Importing session {}...", bundle.session_id);
    let resp = client
        .post(format!("{}/v1/sessions/import", base_url))
        .header("X-Xavier-Token", &token)
        .json(&bundle)
        .send()
        .await?;

    if resp.status().is_success() {
        println!("✅ Imported session {} successfully", bundle.session_id);
    } else {
        println!("❌ Import failed: {}", resp.text().await?);
    }

    Ok(())
}

async fn share_session(session_id: String, peer_node_id: String) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    println!(
        "Sharing session {} with peer {}...",
        session_id, peer_node_id
    );
    let payload = serde_json::json!({
        "peer_node_id": peer_node_id
    });

    let resp = client
        .post(format!("{}/v1/mesh/session/{}/share", base_url, session_id))
        .header("X-Xavier-Token", &token)
        .json(&payload)
        .send()
        .await?;

    if resp.status().is_success() {
        println!("✅ Session shared successfully");
    } else {
        println!("❌ Share failed: {}", resp.text().await?);
    }

    Ok(())
}
