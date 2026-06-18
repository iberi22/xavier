//! System3 reasoning engine module
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod client;
pub mod engine;
pub mod helpers;
#[cfg(test)]
pub mod tests;
pub mod types;

pub use engine::{observe, System3Actor};
pub use types::{
    Action, ActionResult, ActionType, ActorConfig, MemoryOperation, MemoryUpdate, MetaObservations,
    ToolCall,
};
