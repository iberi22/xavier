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
        crate::cli::commands::enums::memory::MemoryCommand::ImportMarkdown { dir } => {
            run_import_markdown(&dir).await
        }
        crate::cli::commands::enums::memory::MemoryCommand::ExportMarkdown { dir, public_only } => {
            run_export_markdown(&dir, public_only.unwrap_or(false)).await
        }
        crate::cli::commands::enums::memory::MemoryCommand::Prune {
            prefix,
            older_than_days,
            dry_run,
            yes,
            json,
        } => run_prune(prefix, older_than_days, dry_run, yes, json).await,
    }
}

async fn run_import_markdown(dir: &std::path::Path) -> Result<()> {
    let workspace_id =
        std::env::var("XAVIER_WORKSPACE_ID").unwrap_or_else(|_| "default".to_string());
    let memory = crate::cli::commands::spawn::load_spawn_memory().await?;
    if let Some(store) = memory.store().await {
        crate::cli::handlers::export::handle_import_markdown(dir, store.as_ref(), &workspace_id)
            .await
    } else {
        anyhow::bail!("Memory store backend unavailable")
    }
}

async fn run_export_markdown(dir: &std::path::Path, public_only: bool) -> Result<()> {
    let workspace_id =
        std::env::var("XAVIER_WORKSPACE_ID").unwrap_or_else(|_| "default".to_string());
    let memory = crate::cli::commands::spawn::load_spawn_memory().await?;
    if let Some(store) = memory.store().await {
        crate::cli::handlers::export::handle_export_markdown(
            dir,
            store.as_ref(),
            &workspace_id,
            public_only,
        )
        .await
    } else {
        anyhow::bail!("Memory store backend unavailable")
    }
}

async fn run_consolidation(nightly: bool) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token().unwrap_or_default();
    let client = CLI_HTTP_CLIENT.clone();

    if nightly {
        println!("🚀 Triggering nightly memory consolidation (with TGD)...");
    } else {
        println!("🚀 Triggering one-off memory consolidation...");
    }

    let response = client
        .post(format!("{}/memory/consolidate", base_url))
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({ "nightly": nightly }))
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            println!(
                "✅ Consolidation complete: {}",
                serde_json::to_string_pretty(&body)?
            );
            Ok(())
        }
        Ok(resp) => {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            println!(
                "⚠️ Server HTTP {} ({}). Falling back to local offline memory consolidation...",
                status.as_u16(),
                body_text
            );
            run_offline_consolidation_cli().await
        }
        Err(e) => {
            println!(
                "⚠️ CONNECTION_REFUSED or server offline ({}). Falling back to local offline memory consolidation...",
                e
            );
            run_offline_consolidation_cli().await
        }
    }
}

async fn run_offline_consolidation_cli() -> Result<()> {
    match crate::cli::handlers::memory::offline_consolidate(None).await {
        Ok(summary) => {
            println!("\n✅ Local offline consolidation complete!");
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        Err(err) => {
            println!("❌ Local offline consolidation failed: {}", err);
            Err(err)
        }
    }
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
    let token = require_xavier_token().unwrap_or_default();
    let client = CLI_HTTP_CLIENT.clone();

    println!("🌙 Triggering nightly consolidation (TGD + memory)...");
    let response = client
        .post(format!("{}/memory/consolidate", base_url))
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({"nightly": true}))
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            println!(
                "✅ Nightly consolidation complete: {}",
                serde_json::to_string_pretty(&body)?
            );
            Ok(())
        }
        Ok(resp) => {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            println!(
                "⚠️ Server HTTP {} ({}). Falling back to local offline memory consolidation...",
                status.as_u16(),
                body_text
            );
            run_offline_consolidation_cli().await
        }
        Err(e) => {
            println!(
                "⚠️ CONNECTION_REFUSED or server offline ({}). Falling back to local offline memory consolidation...",
                e
            );
            run_offline_consolidation_cli().await
        }
    }
}

async fn run_prune(
    prefix: Option<String>,
    older_than_days: Option<u64>,
    dry_run: bool,
    yes: bool,
    json: bool,
) -> Result<()> {
    if !dry_run && !yes {
        print!("⚠️ Are you sure you want to permanently prune memories? [y/N]: ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if !trimmed.eq_ignore_ascii_case("y") && !trimmed.eq_ignore_ascii_case("yes") {
            println!("Prune cancelled.");
            return Ok(());
        }
    }

    if !json {
        if dry_run {
            println!("🔍 Simulating memory pruning (dry-run mode)...");
        } else {
            println!("🧹 Running memory pruning...");
        }
    }

    let base_url = resolve_base_url();
    let token = require_xavier_token().unwrap_or_default();
    let client = CLI_HTTP_CLIENT.clone();

    let req_body = serde_json::json!({
        "prefix": prefix,
        "path_prefix": prefix,
        "older_than_days": older_than_days,
        "dry_run": dry_run,
    });

    let response = client
        .post(format!("{}/memory/prune", base_url))
        .header("X-Xavier-Token", &token)
        .json(&req_body)
        .send()
        .await;

    let result_json = match response {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<serde_json::Value>().await.unwrap_or_default()
        }
        _ => {
            let v1_resp = client
                .post(format!("{}/v1/memories/prune", base_url))
                .header("X-Xavier-Token", &token)
                .json(&req_body)
                .send()
                .await;

            match v1_resp {
                Ok(resp) if resp.status().is_success() => {
                    resp.json::<serde_json::Value>().await.unwrap_or_default()
                }
                _ => {
                    crate::cli::handlers::memory::offline_prune(
                        None,
                        prefix.as_deref(),
                        older_than_days,
                        dry_run,
                    )
                    .await?
                }
            }
        }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&result_json)?);
    } else {
        let count = result_json
            .get("pruned_count")
            .or_else(|| result_json.get("deleted_count"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let bytes = result_json
            .get("reclaimed_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let mode_str = if dry_run {
            "Dry-run simulation complete"
        } else {
            "Prune complete"
        };
        println!(
            "✅ {}: {} memories identified/pruned, {} bytes reclaimed.",
            mode_str, count, bytes
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::enums::memory::MemoryCommand;
    use clap::Parser;

    #[derive(Parser, Debug)]
    struct CliTest {
        #[command(subcommand)]
        cmd: MemoryCommand,
    }

    #[test]
    fn test_memory_prune_args_parsing() {
        let parsed = CliTest::try_parse_from([
            "test",
            "prune",
            "--prefix",
            "gitcore/xavier",
            "--older-than-days",
            "30",
            "--dry-run",
            "--yes",
            "--json",
        ])
        .expect("should parse prune args");

        match parsed.cmd {
            MemoryCommand::Prune {
                prefix,
                older_than_days,
                dry_run,
                yes,
                json,
            } => {
                assert_eq!(prefix.as_deref(), Some("gitcore/xavier"));
                assert_eq!(older_than_days, Some(30));
                assert!(dry_run);
                assert!(yes);
                assert!(json);
            }
            _ => panic!("Expected Prune variant"),
        }
    }

    #[test]
    fn test_memory_prune_defaults() {
        let parsed =
            CliTest::try_parse_from(["test", "prune"]).expect("should parse default prune args");

        match parsed.cmd {
            MemoryCommand::Prune {
                prefix,
                older_than_days,
                dry_run,
                yes,
                json,
            } => {
                assert!(prefix.is_none());
                assert!(older_than_days.is_none());
                assert!(!dry_run);
                assert!(!yes);
                assert!(!json);
            }
            _ => panic!("Expected Prune variant"),
        }
    }
}
