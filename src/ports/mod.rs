//! Port interfaces for hexagonal architecture
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod inbound;
pub mod outbound;

pub use outbound::code_graph;
