//! System scan CLI command handlers
//!
//! Implements system scanning via HTTP API.

use crate::cli::commands::enums::ScanCommand;
use crate::cli::config::{require_xavier_token, resolve_base_url};
use anyhow::Result;

/// Dispatch scan commands
pub async fn handle_scan_command(cmd: ScanCommand) -> Result<()> {
    match cmd {
        ScanCommand::System { format, detailed } => scan_system(format, detailed).await,
        ScanCommand::Security { format } => scan_security(format).await,
    }
}

/// Run system scan
async fn scan_system(_format: String, _detailed: bool) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    println!("Running system scan...\n");

    let url = format!("{}/v1/system/scan", base_url);

    let resp = client
        .get(&url)
        .header("X-Xavier-Token", &token)
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let data: serde_json::Value = r.json().await.unwrap_or_default();
            let ollama_ok = data["ollama"]["running"].as_bool().unwrap_or(false);
            let gpu_detected = data["gpu"]["detected"].as_bool().unwrap_or(false);
            let docker_ok = data["docker"]["running"].as_bool().unwrap_or(false);
            let models = data["ollama"]["models"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);
            let agents = data["cli_agents"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);

            println!("═══════════════════════════════════════════");
            println!("  System Scan Results");
            println!("═══════════════════════════════════════════");
            println!("  Ollama:      {}", if ollama_ok { "Running" } else { "Not running" });
            println!("  Models:      {}", models);
            println!("  GPU:         {}", if gpu_detected { "Detected" } else { "Not detected" });
            println!("  Docker:      {}", if docker_ok { "Running" } else { "Not running" });
            println!("  CLI Agents:  {}", agents);
            println!("═══════════════════════════════════════════");
        }
        _ => {
            println!("Failed to reach system scan endpoint");
        }
    }

    Ok(())
}

/// Run security scan
async fn scan_security(_format: String) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    println!("Running security scan...\n");

    let url = format!("{}/v1/system/security-scan", base_url);

    let resp = client
        .get(&url)
        .header("X-Xavier-Token", &token)
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let data: serde_json::Value = r.json().await.unwrap_or_default();
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        _ => {
            println!("Failed to reach security scan endpoint");
        }
    }

    Ok(())
}
