//! Core memory module for cognitive storage
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod agent_indexer;
pub mod agent_scanner;
pub mod belief_graph;
pub mod bridge;
pub mod checkpoint_summary;
pub mod cloud_sync;
pub mod codex_importer;
pub mod compression;
pub mod connection_provider;
pub mod decay;
pub mod embedder;
pub mod entities;
pub mod entity_graph;
pub mod episodic;
pub mod fallback_store;
pub mod file_indexer;
pub mod graph_store;
pub mod graph_traversal;
pub mod hermes_importer;
pub mod hierarchy;
pub mod jules_importer;
pub mod languages;
pub mod layers_config;
pub mod manager;
pub mod openclaw_indexer;
pub mod openclaw_scanner;
pub mod pack;
pub mod postgres_store;
pub mod qmd;
pub mod qmd_memory;
pub mod query_engine;
pub mod schema;
pub mod semantic;
pub mod semantic_cache;
pub mod simple_index;
pub mod snippet;
pub mod snippet_writethrough;
pub mod sqlite_store;
pub mod sqlite_vec_store;
pub mod store;
pub mod supabase_store;
pub mod sync;
pub mod telemetry;
pub mod virtual_memory;
pub mod working;
pub use connection_provider::*;
pub use fallback_store::*;
pub use query_engine::*;
pub use store::*;

#[cfg(test)]
mod tests;
