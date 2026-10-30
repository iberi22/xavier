//! Provider management CLI command implementation.

use crate::cli::commands::enums::{ProviderCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{require_xavier_token, resolve_base_url};
use anyhow::Result;

/// Handle provider command.
pub async fn handle_provider_command(cmd: ProviderCommand) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    match cmd {
        ProviderCommand::Status { json } => {
            let resp_result = client
                .get(format!("{}/headless/provider/status", base_url))
                .header("X-Xavier-Token", &token)
                .send()
                .await;

            let exit_code = crate::cli::utils::exit_code_for(&resp_result);

            match resp_result {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let status: serde_json::Value = resp.json().await?;
                        println!("{}", serde_json::to_string_pretty(&status)?);
                    } else {
                        let status_code = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        let body_str = if body.is_empty() {
                            "(empty body)"
                        } else {
                            &body
                        };
                        if json {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "error": format!("Failed to get provider status: {} {}", status_code, body_str) })).unwrap());
                        } else {
                            println!(
                                "❌ Failed to get provider status: {} {}",
                                status_code, body_str
                            );
                        }
                    }
                }
                Err(e) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "error": format!("Failed to get provider status: {}", e) })).unwrap());
                    } else {
                        println!("❌ Failed to get provider status: {}", e);
                    }
                }
            }
            std::process::exit(exit_code);
        }
        ProviderCommand::List { json } => {
            let resp_result = client
                .get(format!("{}/v1/providers", base_url))
                .header("X-Xavier-Token", &token)
                .send()
                .await;

            let exit_code = crate::cli::utils::exit_code_for(&resp_result);

            match resp_result {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let list: serde_json::Value = resp.json().await?;
                        if json {
                            println!("{}", serde_json::to_string(&list)?);
                        } else {
                            println!("{}", serde_json::to_string_pretty(&list)?);
                        }
                    } else {
                        println!(
                            "❌ Failed to list providers: {}",
                            resp.text().await.unwrap_or_default()
                        );
                    }
                }
                Err(e) => {
                    println!("❌ Failed to list providers: {}", e);
                }
            }
            std::process::exit(exit_code);
        }
        ProviderCommand::Set { name } => {
            let resp_result = client
                .post(format!("{}/v1/providers/switch", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({ "provider": name }))
                .send()
                .await;

            let exit_code = crate::cli::utils::exit_code_for(&resp_result);

            match resp_result {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let res: serde_json::Value = resp.json().await?;
                        println!(
                            "✅ {}",
                            res["message"].as_str().unwrap_or("Switched provider")
                        );
                    } else {
                        println!(
                            "❌ Failed to set provider: {}",
                            resp.text().await.unwrap_or_default()
                        );
                    }
                }
                Err(e) => {
                    println!("❌ Failed to set provider: {}", e);
                }
            }
            std::process::exit(exit_code);
        }
        ProviderCommand::Auto { strategy: _ } => {
            println!("❌ Route /v1/providers/auto is not available on this daemon version");
            std::process::exit(4);
        }
        ProviderCommand::Fallback { providers } => {
            // Implementation of fallback chain set via HTTP if we add a handler for it,
            // or just inform the user for now.
            println!("Fallback chain management not yet fully implemented via CLI.");
            println!("Target providers: {:?}", providers);
        }
    }

    Ok(())
}
