//! Cloud management handlers for Xavier.

use anyhow::Result;
use colored::*;
use serde_json::json;

use crate::cli::config::{require_xavier_token, resolve_base_url};
use crate::cli::commands::enums::{CloudCommand, CLI_HTTP_CLIENT};
use crate::settings::XavierSettings;

/// Handle cloud commands.
pub async fn handle_cloud_command(cmd: CloudCommand) -> Result<()> {
    match cmd {
        CloudCommand::Status { json } => handle_cloud_status(json).await,
        CloudCommand::SetBackend { backend, json } => handle_cloud_set_backend(backend, json).await,
        CloudCommand::Sync { json } => handle_cloud_sync(json).await,
        CloudCommand::Verify { json } => handle_cloud_verify(json).await,
    }
}

/// Show cloud backend status, connection, and sync stats.
pub async fn handle_cloud_status(as_json: bool) -> Result<()> {
    let settings = XavierSettings::current();
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    // 1. Local backend info
    let local_backend = settings.memory.backend.clone();

    // 2. Cloud info from server
    let cloud_resp = client
        .get(format!("{}/v1/mesh/cloud", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await;

    let cloud_info = if let Ok(resp) = cloud_resp {
        if resp.status().is_success() {
            resp.json::<serde_json::Value>().await.ok()
        } else {
            None
        }
    } else {
        None
    };

    // 3. Stats from server
    let stats_resp = client
        .get(format!("{}/memory/stats", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await;

    let stats = if let Ok(resp) = stats_resp {
        if resp.status().is_success() {
            resp.json::<serde_json::Value>().await.ok()
        } else {
            None
        }
    } else {
        None
    };

    if as_json {
        let output = json!({
            "status": "ok",
            "local_backend": local_backend,
            "cloud_info": cloud_info,
            "stats": stats,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!();
        println!("═══════════════════════════════════════════");
        println!("  {} ", "Cloud Backend Status".bold());
        println!("═══════════════════════════════════════════");
        println!("  {:<15} {}", "Active Backend:", local_backend.cyan());

        if let Some(ci) = &cloud_info {
            let url = ci["url"].as_str().unwrap_or("None");
            let instance = ci["instance_id"].as_str().unwrap_or("None");
            println!("  {:<15} {}", "Cloud URL:", url);
            println!("  {:<15} {}", "Instance ID:", instance);
            println!("  {:<15} {}", "Connection:", "✅ Connected".green());
        } else {
            println!("  {:<15} {}", "Connection:", "⚠️ Disconnected / Not Configured".yellow());
        }

        if let Some(s) = &stats {
            let docs = s["document_count"].as_u64().or(s["total_documents"].as_u64()).unwrap_or(0);
            println!("  {:<15} {}", "Documents:", docs);
        }

        println!("═══════════════════════════════════════════");
    }

    Ok(())
}

async fn handle_cloud_set_backend(backend: String, as_json: bool) -> Result<()> {
    let mut settings = XavierSettings::current();
    let normalized = backend.trim().to_lowercase();

    match normalized.as_str() {
        "sqlite" | "vec" | "supabase" | "postgres" | "auto" => {
            settings.memory.backend = normalized.clone();
            settings.save().await?;

            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "status": "ok",
                        "message": format!("Backend set to {}", normalized),
                        "backend": normalized
                    }))?
                );
            } else {
                println!(
                    "{} Backend successfully set to: {}",
                    "✅".green(),
                    normalized.cyan().bold()
                );
                println!("Note: You may need to restart the Xavier server for changes to take effect.");
            }
        }
        _ => {
            let error_msg = format!(
                "Invalid backend: {}. Supported: sqlite, vec, supabase, postgres, auto",
                backend
            );
            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "status": "error",
                        "message": error_msg
                    }))?
                );
            } else {
                println!("{} {}", "❌".red(), error_msg.red());
            }
        }
    }

    Ok(())
}

async fn handle_cloud_sync(as_json: bool) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    if !as_json {
        println!("{} Triggering cloud synchronization...", "🔄".cyan());
    }

    // Attempting to trigger sync via tasks sync endpoint
    let resp = client
        .post(format!("{}/v1/tasks/sync", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        if as_json {
            println!("{}", serde_json::to_string_pretty(&body)?);
        } else {
            let sync_info = &body["sync"];
            println!(
                "{} Sync triggered successfully!",
                "✅".green()
            );
            if let Some(msg) = sync_info["message"].as_str() {
                println!("  Message: {}", msg);
            }
            println!(
                "  Projects: {}, Tasks: {}",
                sync_info["projects"].as_u64().unwrap_or(0),
                sync_info["tasks"].as_u64().unwrap_or(0)
            );
        }
    } else {
        let error_text = resp.text().await?;
        if as_json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "error",
                    "message": "Sync failed",
                    "detail": error_text
                }))?
            );
        } else {
            println!("{} Sync failed: {}", "❌".red(), error_text);
        }
    }

    Ok(())
}

async fn handle_cloud_verify(as_json: bool) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    if !as_json {
        println!("{} Running full cloud connection health check...", "🔍".cyan());
    }

    let resp = client
        .get(format!("{}/health", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        if as_json {
            println!("{}", serde_json::to_string_pretty(&body)?);
        } else {
            let status = body["status"].as_str().unwrap_or("unknown");
            let service = body["service"].as_str().unwrap_or("xavier");

            println!();
            println!("═══════════════════════════════════════════");
            println!("  {} ", "Cloud Health Check".bold());
            println!("═══════════════════════════════════════════");
            println!("  {:<15} {}", "Service:", service);
            println!(
                "  {:<15} {}",
                "Status:",
                if status == "ok" || status == "healthy" {
                    status.green()
                } else {
                    status.yellow()
                }
            );

            if let Some(checks) = body["checks"].as_array() {
                println!("\n  Details:");
                for check in checks {
                    let name = check["name"].as_str().unwrap_or("?");
                    let c_status = check["status"].as_str().unwrap_or("?");
                    let detail = check["detail"].as_str().unwrap_or("");

                    let status_colored = if c_status == "Pass" || c_status == "healthy" {
                        "PASS".green()
                    } else {
                        "FAIL".red()
                    };

                    println!("    • {:<20} [{}] {}", name, status_colored, detail);
                }
            }
            println!("═══════════════════════════════════════════");
        }
    } else {
        let error_text = resp.text().await?;
        if as_json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "error",
                    "message": "Health check failed",
                    "detail": error_text
                }))?
            );
        } else {
            println!("{} Health check failed: {}", "❌".red(), error_text);
        }
    }

    Ok(())
}
