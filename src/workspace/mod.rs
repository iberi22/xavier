//! Workspace management module
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod config;
pub mod usage;
pub mod ops;
pub mod state;
pub mod registry;
pub mod templates;

pub use config::*;
pub use usage::*;
pub use ops::*;
pub use state::*;
pub use registry::*;
pub use templates::*;

#[cfg(test)]
mod tests;
