//! Plugin system for code-graph.
//!
//! Extracts language-specific parsing into externally-managed plugins so a
//! crashed parser never takes down the indexer. Per the feature spec, parse
//! requests are routed through a per-language **fallback chain**:
//!
//! ```text
//! Plugin → Native (tree-sitter) → NoOp (empty symbols)
//! ```

pub mod engine;
pub mod fallback;
pub mod health;
pub mod manager;
pub mod types;

pub use engine::ProcessEngine;
pub use fallback::FallbackChain;
pub use health::PluginHealthMonitor;
pub use manager::PluginManager;
pub use types::{
    FallbackResolver, FallbackStep, FileToParse, PluginConfig, PluginDescriptor, PluginEngine,
    PluginHealth, PluginRequest, PluginResponse, PluginRegistry, RegistryEntry,
};
