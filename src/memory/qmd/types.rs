//! Type definitions for QMD storage
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

pub use xavier_core_logic::MemoryDocument;

/// Type definitions for the QMD memory system.

#[derive(Debug, Clone)]
pub struct EmbeddingCacheEntry {
    pub vector: Vec<f32>,
    pub cached_at: Instant,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryUsage {
    pub document_count: usize,
    pub storage_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheMetrics {
    pub hits: usize,
    pub misses: usize,
    pub entries: usize,
}

#[derive(Debug, Clone)]
pub struct CachedSearchResult {
    pub documents: Vec<MemoryDocument>,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_doc: bool,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct SearchCacheKey {
    pub workspace_id: String,
    pub query: String,
    pub limit: usize,
    pub filters: String,
}

#[derive(Default)]
pub struct CacheCounters {
    pub hits: AtomicUsize,
    pub misses: AtomicUsize,
}

#[derive(Debug, Clone)]
pub struct QueryBundle {
    pub normalized_query: String,
    pub variants: Vec<String>,
    pub weights: HashMap<String, f32>,
}

impl QueryBundle {
    /// Weight for.
    pub fn weight_for(&self, query: &str) -> f32 {
        self.weights.get(query).copied().unwrap_or(1.0)
    }
}
