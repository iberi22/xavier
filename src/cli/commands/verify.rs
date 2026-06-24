//! CLI verification commands.

use crate::cli::commands::enums::VerifyCommand;
use crate::cli::handlers::system_scan::{
    format_as_json, format_as_markdown, format_as_table, scan_system,
};
use crate::maturity::cli::{MaturityCommand, handle_maturity_command};
use anyhow::Result;

pub async fn handle_verify_command(cmd: VerifyCommand) -> Result<()> {
    match cmd {
        VerifyCommand::Scan { format, detailed } => {
            // Forward to MaturityEngine scan
            handle_maturity_command(MaturityCommand::Scan {
                codebase: None,
                json: format == "json",
                markdown: format == "markdown" || format == "md",
                anchors: None,
                write: false,
            }).await?;
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
