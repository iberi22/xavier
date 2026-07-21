//! CLI verification commands.

use crate::cli::commands::enums::VerifyCommand;
use crate::cli::handlers::system_scan::{
    format_as_json, format_as_markdown, format_as_table, scan_system,
};
use anyhow::Result;

/// Handle verify command.
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
