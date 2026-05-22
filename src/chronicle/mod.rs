//! Chronicle module.
//!
//! Chronicle provides harvesting, redaction, generation, publishing, and CLI
//! primitives for Xavier's daily technical log workflow.
//! Also provides auto-documentation generation from code-graph data.

pub mod auto_docs;
pub mod cli;
pub mod generate;
pub mod harvest;
pub mod patterns;
pub mod prompts;
pub mod publish;
pub mod redact;
pub mod ssg;

pub use redact::{process_output, redact, verify};
