//! MCP (Model Context Protocol) server module
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod auth;
pub mod server;
pub mod session;
pub mod tools_context;
pub mod tools_core;
pub mod tools_memory;
pub mod transport;
pub mod types;

pub use server::*;
pub use session::*;
pub use transport::*;
pub use types::*;
#[cfg(test)]
pub mod regression_token_savings;
#[cfg(test)]
pub mod tests;
