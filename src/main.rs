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

extern crate xavier as xavier_lib;

// Re-export observability module for CLI access
mod observability {
    pub use xavier_lib::observability::*;
}

// Re-export memory types for binary crate access
pub use xavier_lib::crypto;
pub use xavier_lib::kernel;
pub use xavier_lib::memory;
pub use xavier_lib::secrets;
pub use xavier_lib::security;
pub use xavier_lib::storage;
pub use xavier_lib::system;
pub use xavier_lib::telecom;
pub use xavier_lib::workspace;

use anyhow::Result;
use clap::Parser;
use xavier_lib::cli::validate_xavier_data_dir_env;
use xavier_lib::cli::Cli;
use xavier_lib::settings::{self, XavierSettings};

fn main() -> Result<()> {
    std::thread::Builder::new()
        .name("xavier-main".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(8 * 1024 * 1024)
                .build()
                .expect("failed to build Tokio runtime");
            let result = runtime.block_on(async_main());
            // Blocking importers and pool workers must not hold process shutdown open.
            runtime.shutdown_timeout(std::time::Duration::from_secs(3));
            result
        })?
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e))
}

async fn async_main() -> Result<()> {
    // Reject argv credentials before clap can echo an offending argument.
    if std::env::args_os().nth(1).is_some_and(|arg| arg == "llm")
        && std::env::args_os().skip(2).any(|arg| {
            let arg = arg.to_string_lossy();
            arg == "--api-key" || arg.starts_with("--api-key=")
        })
    {
        eprintln!(
            "API keys require --api-key-file PATH or --api-key-stdin; never pass keys in argv"
        );
        std::process::exit(2);
    }
    let cli = Box::new(Cli::parse());
    // Config inspection must work even when startup configuration is invalid.
    if matches!(cli.cmd, Some(xavier_lib::cli::Command::Config(_))) {
        return cli.run().await;
    }
    let llm_config_command = matches!(
        cli.cmd.as_ref(),
        Some(xavier_lib::cli::commands::Command::Llm(_))
            | Some(xavier_lib::cli::commands::Command::Setup { .. })
    );
    if llm_config_command && std::env::var_os("XAVIER_CONFIG_PATH").is_none() {
        // Pin both paths before loading so CLI setters never seed a repo template.
        let config = XavierSettings::resolve_config_path();
        let state = XavierSettings::resolve_runtime_state_path();
        let config = if config.is_absolute() {
            config
        } else {
            std::env::current_dir()?.join(config)
        };
        if std::env::var_os("XAVIER_RUNTIME_STATE_PATH").is_none() {
            std::env::set_var("XAVIER_RUNTIME_STATE_PATH", state);
        }
        std::env::set_var("XAVIER_CONFIG_PATH", config);
    }
    if llm_config_command {
        xavier_lib::cli::commands::llm::capture_environment();
    }
    let loaded_settings = XavierSettings::load()?;
    if let Some(ref settings) = loaded_settings {
        if !llm_config_command {
            if let Err(problems) = settings::validation::validate_local_config(settings) {
                eprintln!("FATAL: Invalid Xavier configuration:");
                for p in &problems {
                    eprintln!("  * {p}");
                }
                eprintln!(
                    "\nFix config/xavier.config.json or run `xavier setup --local` and retry."
                );
                std::process::exit(2);
            }
        }
        settings.apply_to_env();
    }

    // Setup logging
    let log_filter = std::env::var("RUST_LOG")
        .ok()
        .or_else(|| std::env::var("XAVIER_LOG_LEVEL").ok())
        .unwrap_or_else(|| "info".to_string());

    let log_dir = std::path::PathBuf::from(std::env::var("XAVIER_LOG_DIR").unwrap_or_else(|_| {
        // Same state-dir precedence as `cli/server.rs` / `cli/commands/users.rs`:
        // `XAVIER_STATE_DIR`, else `HOME`/`USERPROFILE`, else the current dir.
        // Falling straight to the real `$HOME` here made concurrently-spawned,
        // otherwise-isolated test servers share `$HOME/.xavier/logs` (#2736).
        let state_dir = std::env::var("XAVIER_STATE_DIR")
            .or_else(|_| std::env::var("HOME"))
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        format!("{}/.xavier/logs", state_dir)
    }));

    // Initialize logging
    observability::init_logger(&log_dir, &log_filter);

    // Parse and run CLI
    validate_xavier_data_dir_env()?;
    cli.run().await
}
