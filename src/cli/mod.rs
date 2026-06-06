//! Command-line interface module
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub(crate) mod code_graph;
pub mod commands;
pub(crate) mod config;
pub(crate) mod mcp;
pub mod proxy;
pub(crate) mod security;
pub mod server;
pub mod state;
#[cfg(test)]
mod tests;
pub(crate) mod utils;
pub mod handlers;
pub mod http_setup;
pub mod types;
pub mod websocket;

pub use commands::Command;
pub use state::Cli;
