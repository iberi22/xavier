//! Plugin system for code-graph.
//!
//! Extracts language-specific parsing into externally-managed plugins so a
//! crashed parser never takes down the indexer. Per the feature spec, parse
//! requests are routed through a per-language **fallback chain**:
//!
//! ```text
//! Plugin → Native (tree-sitter) → NoOp (empty symbols)
//! ```
//!
//! Phase scope (this file + siblings):
//! - [`types`]      — descriptors, `FallbackStep`, protocol types, traits.
//! - [`engine`]     — `ProcessEngine` running a plugin as an isolated subprocess.
//! - [`fallback`]   — `FallbackChain` + persistence to `fallback.json`.
//! - [`registry`]   — `PluginRegistry` trait + `GitHubRegistry` / `MockRegistry`.
//! - [`cache`]      — `PluginCache`: archive extraction + version pruning.
//! - [`manager`]    — `PluginManager`: lifecycle (install/update/rollback/uninstall).
//! - [`health`]     — `PluginHealthMonitor`: ring buffer + circuit breaker (F4).
//! - [`discovery`]  — `LanguageDiscovery` (F4).
//!
//! Deferred to later phases: dynamic `from_extension` wiring into the indexer,
//! Prometheus exporter.

pub mod cache;
pub mod discovery;
pub mod engine;
pub mod fallback;
pub mod health;
pub mod manager;
pub mod registry;
pub mod types;

pub use cache::PluginCache;
pub use engine::ProcessEngine;
pub use fallback::FallbackChain;
pub use manager::PluginManager;
pub use registry::{GitHubRegistry, MockRegistry, PluginRegistry, RegistryEntry, RegistryIndex};
pub use types::{
    FallbackResolver, FallbackStep, FileToParse, PluginConfig, PluginDescriptor, PluginEngine,
    PluginHealth, PluginRequest, PluginResponse,
};
