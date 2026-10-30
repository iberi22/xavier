//! CLI usage tracking commands
//!
//! Handles the `xavier usage` subcommand for displaying provider usage
//! statistics and managing manual cooldowns.

use crate::cli::commands::enums::{UsageCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{require_xavier_token, resolve_base_url};
use anyhow::Result;

/// Dispatch a [`UsageCommand`] to the appropriate handler.
pub async fn handle_usage_command(cmd: UsageCommand) -> Result<()> {
    let base_url = resolve_base_url();
    match cmd {
        UsageCommand::Status { json } => {
            let token = require_xavier_token()?;
            let client = CLI_HTTP_CLIENT.clone();
            let providers = ["opencode-go", "deepseek", "groq", "openai", "anthropic"];

            if json {
                let mut results = Vec::new();
                for p in providers {
                    let resp = client
                        .get(format!("{}/v1/usage/status/{}", base_url, p))
                        .header("X-Xavier-Token", &token)
                        .send()
                        .await?;
                    if resp.status().is_success() {
                        let status: serde_json::Value = resp.json().await?;
                        results.push(status);
                    } else {
                        results.push(serde_json::json!({"provider": p, "error": resp.text().await.unwrap_or_default()}));
                    }
                }
                println!("{}", serde_json::to_string_pretty(&results)?);
                return Ok(());
            }

            println!(
                "{:<15} | {:<10} | {:<10} | {:<10} | {:<10} | {:<20}",
                "Provider", "Today", "Weekly", "Monthly", "Cache Hits", "Limited Until"
            );
            println!(
                "{:-<15}-+-{:-<10}-+-{:-<10}-+-{:-<10}-+-{:-<10}-+-{:-<20}",
                "", "", "", "", "", ""
            );
            for p in providers {
                let resp = client
                    .get(format!("{}/v1/usage/status/{}", base_url, p))
                    .header("X-Xavier-Token", &token)
                    .send()
                    .await?;
                if resp.status().is_success() {
                    let status: xavier::agents::rate_limit::QuotaStatus = resp.json().await?;
                    let limited = status
                        .rate_limited_until
                        .map(|u| u.to_rfc3339())
                        .unwrap_or_else(|| "No".to_string());
                    println!(
                        "{:<15} | {:<10} | {:<10} | {:<10} | {:<10} | {:<20}",
                        status.provider,
                        status.used_today,
                        status.used_weekly,
                        status.used_monthly,
                        status.cache_hits,
                        limited
                    );
                }
            }
            Ok(())
        }
        UsageCommand::Update {
            provider,
            percentage,
        } => {
            let token = require_xavier_token()?;
            let client = CLI_HTTP_CLIENT.clone();
            let providers = ["opencode-go", "deepseek", "groq", "openai", "anthropic"];

            if json {
                let mut results = Vec::new();
                for p in providers {
                    let resp = client
                        .get(format!("{}/v1/usage/status/{}", base_url, p))
                        .header("X-Xavier-Token", &token)
                        .send()
                        .await?;
                    if resp.status().is_success() {
                        let status: serde_json::Value = resp.json().await?;
                        results.push(status);
                    } else {
                        results.push(serde_json::json!({"provider": p, "error": resp.text().await.unwrap_or_default()}));
                    }
                }
                println!("{}", serde_json::to_string_pretty(&results)?);
                return Ok(());
            }

            println!(
                "{:<15} | {:<10} | {:<10} | {:<10} | {:<10} | {:<20}",
                "Provider", "Today", "Weekly", "Monthly", "Cache Hits", "Limited Until"
            );
            println!(
                "{:-<15}-+-{:-<10}-+-{:-<10}-+-{:-<10}-+-{:-<10}-+-{:-<20}",
                "", "", "", "", "", ""
            );
            for p in providers {
                let resp = client
                    .get(format!("{}/v1/usage/status/{}", base_url, p))
                    .header("X-Xavier-Token", &token)
                    .send()
                    .await?;
                if resp.status().is_success() {
                    let status: xavier::agents::rate_limit::QuotaStatus = resp.json().await?;
                    let limited = status
                        .rate_limited_until
                        .map(|u| u.to_rfc3339())
                        .unwrap_or_else(|| "No".to_string());
                    println!(
                        "{:<15} | {:<10} | {:<10} | {:<10} | {:<10} | {:<20}",
                        status.provider,
                        status.used_today,
                        status.used_weekly,
                        status.used_monthly,
                        status.cache_hits,
                        limited
                    );
                }
            }
            Ok(())
        }
        UsageCommand::Cooldown { provider, minutes } => {
            let token = require_xavier_token()?;
            let client = CLI_HTTP_CLIENT.clone();
            let providers = ["opencode-go", "deepseek", "groq", "openai", "anthropic"];

            if json {
                let mut results = Vec::new();
                for p in providers {
                    let resp = client
                        .get(format!("{}/v1/usage/status/{}", base_url, p))
                        .header("X-Xavier-Token", &token)
                        .send()
                        .await?;
                    if resp.status().is_success() {
                        let status: serde_json::Value = resp.json().await?;
                        results.push(status);
                    } else {
                        results.push(serde_json::json!({"provider": p, "error": resp.text().await.unwrap_or_default()}));
                    }
                }
                println!("{}", serde_json::to_string_pretty(&results)?);
                return Ok(());
            }

            println!(
                "{:<15} | {:<10} | {:<10} | {:<10} | {:<10} | {:<20}",
                "Provider", "Today", "Weekly", "Monthly", "Cache Hits", "Limited Until"
            );
            println!(
                "{:-<15}-+-{:-<10}-+-{:-<10}-+-{:-<10}-+-{:-<10}-+-{:-<20}",
                "", "", "", "", "", ""
            );
            for p in providers {
                let resp = client
                    .get(format!("{}/v1/usage/status/{}", base_url, p))
                    .header("X-Xavier-Token", &token)
                    .send()
                    .await?;
                if resp.status().is_success() {
                    let status: xavier::agents::rate_limit::QuotaStatus = resp.json().await?;
                    let limited = status
                        .rate_limited_until
                        .map(|u| u.to_rfc3339())
                        .unwrap_or_else(|| "No".to_string());
                    println!(
                        "{:<15} | {:<10} | {:<10} | {:<10} | {:<10} | {:<20}",
                        status.provider,
                        status.used_today,
                        status.used_weekly,
                        status.used_monthly,
                        status.cache_hits,
                        limited
                    );
                }
            }
            Ok(())
        }
    }
}
