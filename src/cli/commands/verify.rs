//! CLI verification commands.

use crate::cli::commands::enums::VerifyCommand;
use crate::cli::handlers::system_scan::{
    format_as_json, format_as_markdown, format_as_table, scan_system,
};
use anyhow::Result;
use std::path::Path;
use xavier_lib::verification::feature_scanner::{
    format_report_json, format_report_table, scan_features,
};

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
        VerifyCommand::Features { path, format } => {
            let root: Option<&Path> = if let Some(ref p) = path {
                Some(Path::new(p))
            } else if let Ok(cwd) = std::env::current_dir() {
                // Keep the PathBuf alive for the duration via leak
                let leaked: &'static Path = Box::leak(cwd.into_boxed_path());
                Some(leaked)
            } else {
                None
            };
            let report = scan_features(root)?;
            match format.as_str() {
                "json" => println!("{}", format_report_json(&report)?),
                _ => println!("{}", format_report_table(&report)),
            }
        }
    }

    Ok(())
}
