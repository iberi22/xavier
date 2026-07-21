//! CLI billing command.

use crate::cli::commands::enums::CLI_HTTP_CLIENT;
use crate::cli::config::{require_xavier_token, resolve_base_url};
use anyhow::Result;

/// Handle billing command.
pub async fn handle_billing_command() -> Result<()> {
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();

    let response = client
        .get(format!("{}/v1/account/usage", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    if status.is_success() {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        println!("Billing request failed ({}):", status);
        println!("{}", serde_json::to_string_pretty(&body)?);
    }

    Ok(())
}
