//! CLI verification commands.

use crate::cli::commands::enums::VerifyCommand;
use crate::cli::handlers::system_scan::{
    format_as_json, format_as_markdown, format_as_table, scan_system,
};
use anyhow::Result;
use std::path::Path;
use xavier_lib::codebase::maturity::engine::MaturityEngine;

pub async fn handle_verify_command(cmd: VerifyCommand) -> Result<()> {
    match cmd {
        VerifyCommand::Scan { format, detailed } => {
            let result = scan_system(detailed).await;
            match format.as_str() {
                "json" => println!("{}", format_as_json(&result)),
                "markdown" | "md" => println!("{}", format_as_markdown(&result)),
                "table" => println!("{}", format_as_table(&result)),
                other => {
                    anyhow::bail!("unsupported verify output format: {other}");
                }
            }
        }
        VerifyCommand::Maturity {
            features,
            format,
            mcp,
        } => {
            let engine = if mcp {
                // Try MCP mode
                MaturityEngine::new_fallback()
            } else {
                MaturityEngine::new_fallback()
            };

            let features_path = features.as_deref().unwrap_or(".xavier/maturity-anchors.json");
            let codebase_root = ".";

            let result = engine
                .analyze("all", Path::new(features_path), Path::new(codebase_root))
                .await?;

            match format.as_str() {
                "json" => println!(
                    "{}",
                    serde_json::to_string_pretty(&result)?
                ),
                _ => {
                    println!("Feature Maturity Report");
                    println!("=====================");
                    println!("Feature: {}", result.feature_id);
                    println!("Score:   {:.1}%", result.score);
                    if !result.gaps.is_empty() {
                        println!("\nGaps:");
                        for gap in &result.gaps {
                            println!("  - {}", gap);
                        }
                    }
                }
            }
        }
        VerifyCommand::Health { format } => {
            crate::cli::handlers::verify::handle_verify_command(VerifyCommand::Health { format })
                .await?;
        }
        VerifyCommand::Save { content } => {
            crate::cli::handlers::verify::handle_verify_command(VerifyCommand::Save { content })
                .await?;
        }
    }

    Ok(())
}
