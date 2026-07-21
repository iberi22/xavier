//! Memory CLI command handlers for consolidation and maintenance.

use crate::cli::commands::enums::CLI_HTTP_CLIENT;
use crate::cli::config::{require_xavier_token, resolve_base_url};
use anyhow::Result;

/// Handle memory command.
pub async fn handle_memory_command(
    cmd: crate::cli::commands::enums::memory::MemoryCommand,
) -> Result<()> {
    match cmd {
        crate::cli::commands::enums::memory::MemoryCommand::Consolidate {
            start,
            stop,
            status,
            nightly,
        } => {
            if start {
                start_consolidation().await
            } else if stop {
                stop_consolidation().await
            } else if status {
                show_consolidation_status().await
            } else if nightly {
                run_nightly_consolidation().await
            } else {
                // Default to one-off consolidation
                run_consolidation(nightly).await
            }
        }
        crate::cli::commands::enums::memory::MemoryCommand::IndexSelf => run_index_self().await,
    }
}

async fn run_consolidation(nightly: bool) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    if nightly {
        println!("🚀 Triggering nightly memory consolidation (with TGD)...");
    } else {
        println!("🚀 Triggering one-off memory consolidation...");
    }

    let resp = client
        .post(format!("{}/memory/consolidate", base_url))
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({ "nightly": nightly }))
        .send()
        .await?;

    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        println!(
            "✅ Consolidation complete: {}",
            serde_json::to_string_pretty(&body)?
        );
    } else {
        println!("❌ Consolidation failed: {}", resp.text().await?);
    }
    Ok(())
}

async fn start_consolidation() -> Result<()> {
    // This would ideally talk to a background daemon control endpoint
    // For now, let's assume it triggers the background job if not running
    println!("🚀 Starting background nightly consolidation scheduler...");
    // Implementation depends on how we expose the scheduler control
    Ok(())
}

async fn stop_consolidation() -> Result<()> {
    println!("🛑 Stopping background nightly consolidation scheduler...");
    Ok(())
}

async fn show_consolidation_status() -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    let resp = client
        .get(format!("{}/v1/system/health", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        if let Some(progress) = body.get("tgd_consolidation") {
            println!("📊 TGD Consolidation Status:");
            println!("{}", serde_json::to_string_pretty(progress)?);
        } else {
            println!("ℹ️ No active consolidation progress found.");
        }
    } else {
        println!("❌ Failed to fetch status: {}", resp.text().await?);
    }
    Ok(())
}

async fn run_index_self() -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    println!("📚 Indexing Xavier's foundational documents...");

    let resp = client
        .post(format!("{}/memory/index-self", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        println!(
            "✅ Self-indexing complete: {}",
            serde_json::to_string_pretty(&body)?
        );
    } else {
        println!("❌ Self-indexing failed: {}", resp.text().await?);
    }
    Ok(())
}

async fn run_nightly_consolidation() -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    println!("🌙 Triggering nightly consolidation (TGD + memory)...");
    let resp = client
        .post(format!("{}/memory/consolidate", base_url))
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({"nightly": true}))
        .send()
        .await?;

    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        println!(
            "✅ Nightly consolidation complete: {}",
            serde_json::to_string_pretty(&body)?
        );
    } else {
        println!("❌ Nightly consolidation failed: {}", resp.text().await?);
    }
    Ok(())
}
