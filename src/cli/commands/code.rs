// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! CLI code graph query command
//!
//! Handles the `xavier code` subcommand which queries Xavier's code graph
//! for symbol discovery, dependency analysis, and complexity metrics.

use crate::cli::commands::enums::{CodeCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{require_xavier_token, resolve_base_url};

use anyhow::Result;

/// Dispatch a [`CodeCommand`] by making the appropriate HTTP request to the Xavier server.
pub async fn handle_code_command(cmd: CodeCommand) -> Result<()> {
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();

    let response = match cmd {
        CodeCommand::Scan { path } => {
            client
                .post(format!("{}/code/scan", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({ "path": path }))
                .send()
                .await?
        }
        CodeCommand::Find { query, limit, kind } => {
            client
                .post(format!("{}/code/find", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({ "query": query, "limit": limit, "kind": kind }))
                .send()
                .await?
        }
        CodeCommand::Dependencies {
            query,
            depth,
            limit,
            edge_type,
        } => {
            client
                .post(format!("{}/code/dependencies", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({
                    "query": query,
                    "depth": depth,
                    "limit": limit,
                    "edge_type": edge_type
                }))
                .send()
                .await?
        }
        CodeCommand::ReverseDependencies {
            query,
            depth,
            limit,
            edge_type,
        } => {
            client
                .post(format!("{}/code/reverse-dependencies", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({
                    "query": query,
                    "depth": depth,
                    "limit": limit,
                    "edge_type": edge_type
                }))
                .send()
                .await?
        }
        CodeCommand::CallChain {
            query,
            depth,
            limit,
        } => {
            client
                .post(format!("{}/code/call-chain", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({
                    "query": query,
                    "depth": depth,
                    "limit": limit
                }))
                .send()
                .await?
        }
        CodeCommand::Hubs => {
            client
                .get(format!("{}/code/hubs", base_url))
                .header("X-Xavier-Token", &token)
                .send()
                .await?
        }
        CodeCommand::Hotspots => {
            client
                .get(format!("{}/code/hotspots", base_url))
                .header("X-Xavier-Token", &token)
                .send()
                .await?
        }
        CodeCommand::Stats => {
            client
                .get(format!("{}/code/stats", base_url))
                .header("X-Xavier-Token", &token)
                .send()
                .await?
        }
        CodeCommand::Dump { path } => {
            client
                .post(format!("{}/code/dump", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({ "path": path }))
                .send()
                .await?
        }
        CodeCommand::Load { path } => {
            client
                .post(format!("{}/code/load", base_url))
                .header("X-Xavier-Token", &token)
                .json(&serde_json::json!({ "path": path }))
                .send()
                .await?
        }
    };

    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    if status.is_success() {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        println!("Code graph request failed ({}):", status);
        println!("{}", serde_json::to_string_pretty(&body)?);
    }
    Ok(())
}
