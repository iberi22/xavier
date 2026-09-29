//! Xavier glue for the standalone `xavier-pageindex` crate: env settings, a
//! lazily opened shared index and protocol-agnostic tool logic (MCP/HTTP
//! adapters live elsewhere and only translate to/from JSON).

pub mod settings;
pub mod state;
pub mod summarizer;
pub mod tools;

pub use settings::PageIndexSettings;
pub use state::PageIndexState;
pub use tools::{call, tool_specs, ToolContext, ToolSpec};

#[cfg(test)]
mod parity_tests;
