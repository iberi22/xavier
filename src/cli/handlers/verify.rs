// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Verify CLI command handlers
//!
//! Implements system health check and memory verification via HTTP API.

use crate::cli::commands::enums::VerifyCommand;
use crate::cli::config::{require_xavier_token, resolve_base_url};
use anyhow::Result;

/// Dispatch verify commands
pub async fn handle_verify_command(cmd: VerifyCommand) -> Result<()> {
    match cmd {
        VerifyCommand::Scan { .. } => {
            anyhow::bail!("verify scan is handled by the command dispatcher")
        }
        VerifyCommand::Health { format } => verify_health(format).await,
        VerifyCommand::Save { content } => verify_save(content).await,
    }
}

/// Run full system health check
async fn verify_health(format: String) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    println!("Running health check on {}...", base_url);

    let resp = client
        .get(format!("{}/v1/system/health", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    match format.as_str() {
        "json" => {
            let data: serde_json::Value = if resp.status().is_success() {
                resp.json().await.unwrap_or_default()
            } else {
                serde_json::json!({"status": "error", "error": resp.text().await.unwrap_or_default()})
            };
            let pretty = serde_json::to_string_pretty(&data)?;
            println!("{}", pretty);
        }
        "markdown" => {
            let data: serde_json::Value = if resp.status().is_success() {
                resp.json().await.unwrap_or_default()
            } else {
                serde_json::json!({"status": "unreachable"})
            };
            let status = data["status"].as_str().unwrap_or("unknown");
            let uptime = data["uptime_secs"].as_u64().unwrap_or(0);
            let mem = data["memory_mb"].as_u64().unwrap_or(0);
            let version = data["version"]
                .as_str()
                .unwrap_or(env!("CARGO_PKG_VERSION"));

            println!("# System Health");
            println!();
            println!("- Status: **{}**", status);
            println!("- Version: {}", version);
            println!("- Uptime: {}s", uptime);
            println!("- Memory: {} MB", mem);
        }
        _ => {
            if resp.status().is_success() {
                let data: serde_json::Value = resp.json().await.unwrap_or_default();
                let status = data["status"].as_str().unwrap_or("unknown");

                let status_icon = match status {
                    "ok" | "healthy" => "[OK]",
                    "degraded" => "[!]",
                    _ => "[X]",
                };

                println!();
                println!("{}", "=".repeat(47));
                println!("  System Health Check");
                println!("{}", "=".repeat(47));
                println!("  Status:      {} {}", status_icon, status);

                if let Some(uptime) = data["uptime_secs"].as_u64() {
                    println!("  Uptime:      {}s ({}m)", uptime, uptime / 60);
                }
                if let Some(mem) = data["memory_mb"].as_u64() {
                    println!("  Memory:      {} MB", mem);
                }
                if let Some(agents) = data["active_agents"].as_u64() {
                    println!("  Agents:      {}", agents);
                }
                if let Some(alerts) = data["alerts"].as_array() {
                    if !alerts.is_empty() {
                        println!("  Alerts:      {}", alerts.len());
                        for alert in alerts {
                            println!("    [*]  {}", alert.as_str().unwrap_or("?"));
                        }
                    } else {
                        println!("  Alerts:      [OK] None");
                    }
                }
                println!("{}", "=".repeat(47));
            } else {
                println!("[X] Health check failed: {}", resp.text().await?);
            }
        }
    }

    Ok(())
}

/// Verify memory save/retrieve round-trip
async fn verify_save(content: String) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    let start = std::time::Instant::now();

    println!("Running memory save/retrieve verification...");

    let save_payload = serde_json::json!({
        "content": content,
        "title": "CLI Verification Test",
        "path": "/_cli_verify/test_content"
    });

    let save_resp = client
        .post(format!("{}/xavier/verify/save", base_url))
        .header("X-Xavier-Token", &token)
        .json(&save_payload)
        .send()
        .await?;

    let elapsed = start.elapsed();

    if save_resp.status().is_success() {
        let result: serde_json::Value = save_resp.json().await.unwrap_or_default();
        let save_ok = result["save_ok"].as_bool().unwrap_or(false);
        let retrieve_ok = result["retrieve_ok"].as_bool().unwrap_or(false);
        let match_score = result["match_score"].as_f64().unwrap_or(0.0);
        let latency = result["latency_ms"]
            .as_u64()
            .unwrap_or(elapsed.as_millis() as u64);

        println!();
        println!("{}", "=".repeat(47));
        println!("  Memory Verification Result");
        println!("{}", "=".repeat(47));
        println!(
            "  Save:      {}",
            if save_ok { "[OK] OK" } else { "[X] Failed" }
        );
        println!(
            "  Retrieve:  {}",
            if retrieve_ok { "[OK] OK" } else { "[X] Failed" }
        );
        println!("  Match:     {:.1}%", match_score * 100.0);
        println!("  Latency:   {} ms", latency);
        println!("{}", "=".repeat(47));

        if save_ok && retrieve_ok {
            println!("[OK] Verification passed!");
        } else {
            println!("[X] Verification failed!");
        }
    } else {
        println!(
            "[X] Verification request failed: {}",
            save_resp.text().await?
        );
    }

    Ok(())
}
