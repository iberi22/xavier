//! Core memory module for cognitive storage
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod agent_indexer;
pub mod agent_scanner;
pub mod belief_graph;
pub mod bridge;
pub mod checkpoint_summary;
pub mod engram_bridge;
pub mod cloud_sync;
pub mod decay;
pub mod embedder;
pub mod entities;
pub mod entity_graph;
pub mod episodic;
pub mod file_indexer;
pub mod graph_store;
pub mod graph_traversal;
pub mod hierarchy;
pub mod languages;
pub mod layers_config;
pub mod manager;
pub mod openclaw_indexer;
pub mod openclaw_scanner;
pub mod pack;
pub mod postgres_store;
pub mod qmd;
pub mod qmd_memory;
pub mod schema;
pub mod semantic;
pub mod semantic_cache;
pub mod simple_index;
pub mod sqlite_store;
pub mod sqlite_vec_store;
pub mod store;
pub mod supabase_store;
pub mod sync;
pub mod telemetry;
pub mod virtual_memory;
pub mod working;
pub use store::*;

#[cfg(test)]
mod tests;
