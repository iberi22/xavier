//! Command-line interface module
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod commands;
pub(crate) mod config;
pub mod handlers;
pub mod http_setup;
pub(crate) mod mcp;
pub mod code_dump;
pub mod onboarding;
pub mod proxy;
pub(crate) mod security;
pub mod server;
pub mod state;
#[cfg(test)]
mod tests;
pub mod types;
pub(crate) mod utils;
pub mod websocket;

pub use commands::Command;
pub use state::Cli;
