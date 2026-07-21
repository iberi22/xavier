// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! CLI token generation commands
//!
//! Handles the `xavier token` subcommand for generating random tokens
//! and signed HMAC tokens for users.

use crate::cli::commands::enums::TokenCommand;
use anyhow::Result;

/// Dispatch a [`TokenCommand`].
pub async fn handle_token_command(cmd: TokenCommand) -> Result<()> {
    match cmd {
        TokenCommand::New => {
            use rand::RngCore;
            let mut bytes = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut bytes);
            let token = xavier::utils::crypto::hex_encode(&bytes);
            println!("New random token generated:");
            println!("{}", token);
            eprintln!("\nAdd this to your env: set XAVIER_TOKEN={}", token);
        }
        TokenCommand::Gen { user_id } => {
            if user_id.trim().is_empty() {
                anyhow::bail!("user_id must not be empty");
            }
            let manager = xavier::security::SecurityManager::new();
            let token = manager.generate_token(&user_id).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to generate HMAC token: {}. Ensure XAVIER_TOKEN_SECRET is set.",
                    e
                )
            })?;
            println!("Signed HMAC token for {}:", user_id);
            println!("{}", token);
        }
    }
    Ok(())
}
