//! Binary entry point for the Xavier cognitive memory system
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    clippy::bool_comparison,
    clippy::clone_on_copy,
    clippy::derivable_impls,
    clippy::field_reassign_with_default,
    clippy::needless_range_loop,
    clippy::single_match,
    clippy::type_complexity
)]

// Xavier - Cognitive Memory System
// Public open-core release

mod cli;
mod settings;
extern crate xavier as xavier_lib;

// Re-export observability module for CLI access
mod observability {
    pub use xavier_lib::observability::*;
}

// Re-export memory types for binary crate access
pub use xavier_lib::memory;
pub use xavier_lib::secrets;
pub use xavier_lib::workspace;

use crate::settings::XavierSettings;
use anyhow::Result;
use clap::Parser;
use cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    let loaded_settings = XavierSettings::load()?;
    if let Some(ref settings) = loaded_settings {
        if let Err(problems) = crate::settings::validation::validate_local_config(settings) {
            eprintln!("FATAL: Invalid Xavier configuration:");
            for p in &problems {
                eprintln!("  * {p}");
            }
            eprintln!("\nFix config/xavier.config.json or run `xavier setup --local` and retry.");
            std::process::exit(2);
        }
        settings.apply_to_env();
    }

    // Setup logging
    let log_filter = std::env::var("RUST_LOG")
        .ok()
        .or_else(|| std::env::var("XAVIER_LOG_LEVEL").ok())
        .unwrap_or_else(|| "info".to_string());

    let log_dir = std::path::PathBuf::from(std::env::var("XAVIER_LOG_DIR").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| ".".to_string());
        format!("{}/.xavier/logs", home)
    }));

    // Initialize logging
    observability::init_logger(&log_dir, &log_filter);

    // Parse and run CLI
    let cli = Cli::parse();
    cli.run().await
}
