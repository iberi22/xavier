//! Workspace management module
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod config;
pub mod ops;
pub mod registry;
pub mod state;
pub mod templates;
pub mod usage;

pub use config::*;
pub use ops::*;
pub use registry::*;
pub use state::*;
pub use templates::*;
pub use usage::*;

#[cfg(test)]
mod tests;
