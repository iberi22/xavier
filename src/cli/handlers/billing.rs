//! Billing CLI command handlers
//!
//! Implements billing status, plans, and invoice via HTTP API.

use crate::cli::commands::enums::BillingCommand;
use crate::cli::config::{require_xavier_token, resolve_base_url};
use anyhow::Result;

/// Dispatch billing commands
pub async fn handle_billing_command(cmd: BillingCommand) -> Result<()> {
    match cmd {
        BillingCommand::Status => billing_status().await,
        BillingCommand::Plans => billing_plans().await,
        BillingCommand::Invoice { period, format } => billing_invoice(period, format).await,
    }
}

/// Show current billing status / subscription info
async fn billing_status() -> Result<()> {
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
        println!("{}", "=".repeat(47));
        println!("  Billing Status");
        println!("{}", "=".repeat(47));
        println!("  Plan:         {}", plan);
        println!("  Storage:      {} GB", max_storage);
        println!("  Max Nodes:    {}", max_nodes);
        println!("  Features:     {}", features);
        println!(
            "  Stripe:       {}",
            if status["status"] == "ok" {
                "Configured"
            } else {
                "Not configured (using Free tier)"
            }
        );
        println!("{}", "=".repeat(47));
    } else {
        println!();
        println!("{}", "=".repeat(47));
        println!("  Billing Status (offline/default)");
        println!("{}", "=".repeat(47));
        println!("  Plan:         free");
        println!("  Storage:      0 GB (local only)");
        println!("  Max Nodes:    0 (local only)");
        println!("  Features:     local_only, basic_memory");
        println!("  Stripe:       Not configured");
        println!("{}", "=".repeat(47));
    }

    Ok(())
}

/// List available billing plans
async fn billing_plans() -> Result<()> {
    println!("Available plans (static listing):");
    println!("  - Free (local only, limited storage)");
    println!("  - Pro (cloud sync, 10GB, 5 agents)");
    println!("  - Team (cloud sync, 100GB, unlimited agents)");

    Ok(())
}

/// Generate or show invoice summary
async fn billing_invoice(period: String, _format: String) -> Result<()> {
    println!("Invoice for period: {}", period);
    println!("(billing invoice generation not yet implemented)");

    Ok(())
}
