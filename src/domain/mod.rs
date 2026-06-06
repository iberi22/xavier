//! Domain module for core business logic
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod agent;
pub mod audit;
pub mod belief;
pub mod error;
pub mod memory;
pub mod pattern;
pub mod proxy;
pub mod security;

pub use error::AppError;
