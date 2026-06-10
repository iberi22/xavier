//! Memory Garbage Collector
//!
//! Cleans up orphaned vectors, empty documents, and stale entries
//! from the memory store and vector index.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

use super::core::MemoryManager;

/// Statistics from a garbage collection run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GCStats {
    /// Number of empty documents removed
    pub empty_docs_removed: usize,
    /// Number of documents with zero-length content cleaned
    pub zero_content_removed: usize,
    /// Total bytes freed
    pub bytes_freed: u64,
    /// Number of stale metadata entries cleaned
    pub stale_metadata_cleaned: usize,
    /// Duration of the GC run in milliseconds
    pub duration_ms: u64,
    /// Number of orphaned vectors cleaned from the backend
    pub orphaned_vectors_cleaned: usize,
}

impl MemoryManager {
    /// Run a full garbage collection cycle.
    ///
    /// 1. Remove documents with empty content
    /// 2. Remove documents with no ID
    /// 3. Clean stale tracking entries (access times, relevance scores)
    pub async fn garbage_collect(&self) -> Result<GCStats> {
        let start = std::time::Instant::now();
        let mut stats = GCStats::default();

        let docs = self.memory.all_documents().await;
        let mut valid_ids = std::collections::HashSet::new();

        // Pass 1: Remove empty/broken documents
        for doc in &docs {
            let Some(doc_id) = &doc.id else {
                continue;
            };

            let is_empty = doc.content.trim().is_empty();
            let is_zero = doc.content.is_empty() && doc.metadata == serde_json::Value::Null;

            if is_empty || is_zero {
                let size = doc.estimated_bytes();
                if self.memory.delete(doc_id).await?.is_some() {
                    stats.bytes_freed += size;
                    if is_empty {
                        stats.empty_docs_removed += 1;
                    }
                    if is_zero {
                        stats.zero_content_removed += 1;
                    }
                }
            } else {
                valid_ids.insert(doc_id.clone());
            }
        }

        // Pass 2: Clean stale tracking entries
        {
            let mut access_times = self
                .last_access_times
                .lock()
                .expect("gc: last_access_times lock poisoned");
            let before = access_times.len();
            access_times.retain(|id, _| valid_ids.contains(id));
            stats.stale_metadata_cleaned += before - access_times.len();
        }

        {
            let mut relevance = self
                .relevance_scores
                .lock()
                .expect("gc: relevance_scores lock poisoned");
            let before = relevance.len();
            relevance.retain(|id, _| valid_ids.contains(id));
            stats.stale_metadata_cleaned += before - relevance.len();
        }

        {
            let mut created = self
                .created_times
                .lock()
                .expect("gc: created_times lock poisoned");
            let before = created.len();
            created.retain(|id, _| valid_ids.contains(id));
            stats.stale_metadata_cleaned += before - created.len();
        }

        // Pass 3: Backend specific cleanup (orphaned vectors)
        if let Some(store) = self.memory.store().await {
            if let Ok(orphans) = store.cleanup_orphans().await {
                stats.orphaned_vectors_cleaned = orphans;
            }
        }

        stats.duration_ms = start.elapsed().as_millis() as u64;

        info!(
            empty_removed = stats.empty_docs_removed,
            zero_removed = stats.zero_content_removed,
            stale_cleaned = stats.stale_metadata_cleaned,
            orphans_cleaned = stats.orphaned_vectors_cleaned,
            bytes_freed = stats.bytes_freed,
            duration_ms = stats.duration_ms,
            "Garbage collection complete"
        );

        Ok(stats)
    }
}
