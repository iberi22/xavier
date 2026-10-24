//! Context assembly for on-demand code explanations.
//!
//! Gathers evidence about a code fragment from Xavier's own sources. Rendering
//! the explanation (LLM, HTTP route) lives outside this module.

pub mod context;
pub mod history;
pub mod memory;

pub use context::{
    assemble, Evidence, EvidenceKind, EvidenceSource, ExplainContext, ExplainTarget,
};
pub use history::GitHistorySource;
pub use memory::{MemoryHit, MemorySearch, MemorySource, QmdMemorySearch};
