//! HTTP inbound adapter and REST API implementation
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod dto;
pub mod routes;
pub mod state;
pub mod time_metrics_adapter;

#[cfg(feature = "enterprise")]
pub mod plugins;

pub use state::AppState;
pub mod handlers;
