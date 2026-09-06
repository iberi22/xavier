// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 SouthWest AI Labs (SWAL)
//
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
pub use xavier_lib::kernel;
pub use xavier_lib::memory;
pub use xavier_lib::secrets;
pub use xavier_lib::workspace;

use crate::settings::XavierSettings;
use anyhow::Result;
use clap::Parser;
use cli::config::validate_xavier_data_dir_env;
use cli::Cli;

fn main() -> Result<()> {
    std::thread::Builder::new()
        .name("xavier-main".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(8 * 1024 * 1024)
                .build()
                .expect("failed to build Tokio runtime")
                .block_on(async_main())
        })?
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e))
}

async fn async_main() -> Result<()> {
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
    validate_xavier_data_dir_env()?;
    let cli = Box::new(Cli::parse());
    cli.run().await
}
