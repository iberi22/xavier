//! Wallet CLI commands [SKELETON — decisions pending with BELA]

use crate::cli::commands::enums::WalletCommand;
use anyhow::Result;

pub async fn handle_wallet_command(cmd: WalletCommand) -> Result<()> {
    match cmd {
        WalletCommand::Balance => {
            println!(
                "[SKELETON] Wallet balance display — pending BELA decisions on tokenomics model."
            );
            println!("  Use `xavier wallet balance` to check your XP balance once implemented.");
        }
        WalletCommand::Transactions { limit } => {
            println!(
                "[SKELETON] Wallet transactions — pending BELA decisions on tokenomics model."
            );
            println!("  Showing last {limit} transactions (placeholder).");
        }
    }
    Ok(())
}
