//! Billing CLI command handlers
//!
//! Implements billing status and invoice via HTTP API.

use crate::cli::config::{require_xavier_token, resolve_base_url};
use anyhow::Result;

/// Show current billing status / subscription info
pub async fn handle_billing_command() -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    println!("Fetching billing status from {}...", base_url);

    let resp = client
        .get(format!("{}/v1/billing/status", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let status: serde_json::Value = resp.json().await?;

        let plan = status["subscription"]["plan"].as_str().unwrap_or("free");
        let max_storage = status["subscription"]["limits"]["max_storage_gb"]
            .as_u64()
            .unwrap_or(0);
        let max_nodes = status["subscription"]["limits"]["max_nodes"]
            .as_u64()
            .unwrap_or(0);
        let features = status["subscription"]["limits"]["features"]
            .as_array()
            .map(|f| {
                f.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();

        println!();
        println!("═══════════════════════════════════════════");
        println!("  Billing Status");
        println!("═══════════════════════════════════════════");
        println!("  Plan:         {}", plan);
        println!("  Storage:      {} GB", max_storage);
        println!("  Max Nodes:    {}", max_nodes);
        println!("  Features:     {}", features);
        println!(
            "  Stripe:       {}",
            if status["status"] == "ok" {
                "✅ Configured"
            } else {
                "⚠️ Not configured (using Free tier)"
            }
        );
        println!("═══════════════════════════════════════════");
    } else {
        println!();
        println!("═══════════════════════════════════════════");
        println!("  Billing Status (offline/default)");
        println!("═══════════════════════════════════════════");
        println!("  Plan:         free");
        println!("  Storage:      0 GB (local only)");
        println!("  Max Nodes:    0 (local only)");
        println!("  Features:     local_only, basic_memory");
        println!("  Stripe:       ❌ Not configured");
        println!("═══════════════════════════════════════════");
    }

    Ok(())
}
