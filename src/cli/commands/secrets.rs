//! CLI commands for secret/ephemeral credential management (Clavis)
//!
//! Handles the `xavier secrets` subcommand for lending, listing, and
//! revoking ephemeral secret leases to/from agents.

use crate::cli::commands::enums::{SecretsCommand, VaultCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{resolve_base_url, xavier_token};
use xavier::secrets::vault::HardwareVault;

use anyhow::Result;

/// Dispatch a [`VaultCommand`] to the appropriate handler.
pub async fn handle_vault_command(cmd: VaultCommand) -> Result<()> {
    let vault = HardwareVault::new("xavier");
    match cmd {
        VaultCommand::Set { key, value } => {
            vault.store_secret(&key, &value)?;
            println!("Secret '{}' stored in hardware vault.", key);
        }
        VaultCommand::Get { key } => {
            let value = vault.get_secret(&key)?;
            println!("{}: {}", key, value);
        }
        VaultCommand::Delete { key } => {
            vault.delete_secret(&key)?;
            println!("Secret '{}' deleted from hardware vault.", key);
        }
    }
    Ok(())
}

/// Dispatch a [`SecretsCommand`] to the appropriate handler.
pub async fn handle_secrets_command(cmd: SecretsCommand) -> Result<()> {
    match cmd {
        SecretsCommand::Lend {
            secret_name,
            agent,
            ttl,
        } => lend_secret(&secret_name, &agent, ttl).await,
        SecretsCommand::ListLeases => list_leases().await,
        SecretsCommand::Revoke { token } => revoke_lease(&token).await,
        SecretsCommand::Status { token } => check_lease_status(&token).await,
    }
}

async fn lend_secret(name: &str, agent: &str, ttl: u64) -> Result<()> {
    let token = xavier_token();
    let url = format!("{}/secrets/lend", resolve_base_url());
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .post(&url)
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({
            "secret_name": name,
            "agent_id": agent,
            "ttl_seconds": ttl
        }))
        .send()
        .await?;

    if response.status().is_success() {
        let body: serde_json::Value = response.json().await?;
        println!("Secret lent successfully!");
        println!("Lease Token: {}", body["token"]);
        println!("Expires: {}", body["expires_at"]);
    } else {
        println!("Failed to lend secret: {}", response.status());
    }
    Ok(())
}

async fn list_leases() -> Result<()> {
    let token = xavier_token();
    let url = format!("{}/secrets/leases", resolve_base_url());
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .get(&url)
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if response.status().is_success() {
        let leases: Vec<serde_json::Value> = response.json().await?;
        println!(
            "{:<20} {:<20} {:<20} {:<10}",
            "Agent", "Secret", "Expires", "Status"
        );
        for lease in leases {
            println!(
                "{:<20} {:<20} {:<20} {:<10}",
                lease["agent_id"].as_str().unwrap_or("?"),
                lease["secret_name"].as_str().unwrap_or("?"),
                lease["expires_at"].as_str().unwrap_or("?"),
                if lease["revoked"].as_bool().unwrap_or(false) {
                    "Revoked"
                } else {
                    "Active"
                }
            );
        }
    } else {
        println!("Failed to list leases: {}", response.status());
    }
    Ok(())
}

async fn revoke_lease(token_str: &str) -> Result<()> {
    let token = xavier_token();
    let url = format!("{}/secrets/revoke", resolve_base_url());
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .post(&url)
        .header("X-Xavier-Token", &token)
        .json(&serde_json::json!({ "token": token_str }))
        .send()
        .await?;

    if response.status().is_success() {
        println!("Lease revoked successfully.");
    } else {
        println!("Failed to revoke lease: {}", response.status());
    }
    Ok(())
}

async fn check_lease_status(token_str: &str) -> Result<()> {
    let token = xavier_token();
    let url = format!("{}/secrets/status/{}", resolve_base_url(), token_str);
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .get(&url)
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if response.status().is_success() {
        let status: serde_json::Value = response.json().await?;
        println!(
            "Lease Status: {}",
            if status["revoked"].as_bool().unwrap_or(false) {
                "Revoked"
            } else {
                "Active"
            }
        );
        println!("Agent: {}", status["agent_id"]);
        println!("Expires: {}", status["expires_at"]);
    } else {
        println!("Failed to get lease status: {}", response.status());
    }
    Ok(())
}
