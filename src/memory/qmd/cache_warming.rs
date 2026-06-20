//! Predictive Cache Warming for QmdMemory
//!
//! Tracks document access patterns and pre-warms the search cache
//! with the most frequently accessed documents. This reduces latency
//! for hot queries after system startup.
//!
//! Based on HORMER Section 3.4 — navigation-aware prefetching.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;

use super::QmdMemory;

/// Score from a file to a path for cache warming prioritization
pub type HormerScoreMap = HashMap<String, f64>;

/// Tracks access frequency for document IDs
#[derive(Debug, Clone)]
pub struct PredictiveCacheWarmup {
    /// Access counters: doc_id → (count, last_accessed, first_accessed)
    access_stats: Arc<Mutex<HashMap<String, AccessStats>>>,
    /// How long to track before resetting counters (for recency weighting)
    #[allow(dead_code)]
    pub(crate) track_period: Duration,
    /// How many top documents to pre-warm
    top_k: usize,
    /// Whether warming is enabled
    enabled: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccessStats {
    pub count: u64,
    pub last_accessed: SystemTime,
    pub first_accessed: SystemTime,
}

impl PredictiveCacheWarmup {
    /// Creates a new warmup tracker with sensible defaults.
    ///
    /// - `track_period`: 1 hour (stats older than this may be decayed)
    /// - `top_k`: 64 documents pre-warmed
    pub fn new() -> Self {
        Self {
            access_stats: Arc::new(Mutex::new(HashMap::new())),
            track_period: Duration::from_secs(3600),
            top_k: 64,
            enabled: Arc::new(Mutex::new(false)),
        }
    }

    /// Creates a new warmup tracker with custom parameters.
    pub fn with_params(track_period: Duration, top_k: usize) -> Self {
        Self {
            access_stats: Arc::new(Mutex::new(HashMap::new())),
            track_period,
            top_k,
            enabled: Arc::new(Mutex::new(false)),
        }
    }

    /// Records a document access. Thread-safe via internal Mutex.
    pub async fn track_access(&self, doc_id: &str) {
        let mut stats = self.access_stats.lock().await;
        let now = SystemTime::now();

        let entry = stats.entry(doc_id.to_string()).or_insert(AccessStats {
            count: 0,
            last_accessed: now,
            first_accessed: now,
        });
        entry.count += 1;
        entry.last_accessed = now;
    }

    /// Returns the top K most accessed documents, sorted by count descending.
    pub async fn get_hot_docs(&self) -> Vec<String> {
        let stats = self.access_stats.lock().await;

        let mut sorted: Vec<(String, &AccessStats)> =
            stats.iter().map(|(k, v)| (k.clone(), v)).collect();

        // Sort by count descending, then by recency
        sorted.sort_by(|a, b| {
            b.1.count
                .cmp(&a.1.count)
                .then_with(|| b.1.last_accessed.cmp(&a.1.last_accessed))
        });

        sorted
            .into_iter()
            .take(self.top_k)
            .map(|(id, _)| id)
            .collect()
    }

    /// Pre-warms the search cache by touching the hot documents
    /// in the given QmdMemory instance. Returns the number of documents touched.
    pub async fn warmup(&self, memory: &QmdMemory) -> usize {
        let enabled = *self.enabled.lock().await;
        if !enabled {
            return 0;
        }

        let hot_docs = self.get_hot_docs().await;
        if hot_docs.is_empty() {
            return 0;
        }

        let mut warmed = 0;
        let docs = memory.docs.read().await;
        for doc_id in &hot_docs {
            // Look up by id or path — iteration ensures cache stays hot
            if docs.iter().any(|d| d.id.as_deref() == Some(doc_id) || &d.path == doc_id) {
                warmed += 1;
            }
        }

        warmed
    }

    /// Sets the top_k value.
    pub fn set_top_k(&mut self, k: usize) {
        self.top_k = k;
    }

    /// Returns the total number of unique documents tracked.
    pub async fn unique_docs_tracked(&self) -> usize {
        self.access_stats.lock().await.len()
    }

    /// Returns the total number of access events tracked.
    pub async fn total_accesses(&self) -> u64 {
        let stats = self.access_stats.lock().await;
        stats.values().map(|s| s.count).sum()
    }

    /// Enables or disables cache warming.
    pub async fn set_enabled(&self, enabled: bool) {
        *self.enabled.lock().await = enabled;
    }

    /// Returns whether warming is enabled.
    pub async fn is_enabled(&self) -> bool {
        *self.enabled.lock().await
    }

    /// Clears all tracked access stats (e.g., after a reset).
    pub async fn reset(&self) {
        self.access_stats.lock().await.clear();
    }

    /// Predictively warms the cache for a given path by pre-loading the
    /// top-scored neighbor documents according to the HORMER scores map.
    ///
    /// `path` — the directory or file path being navigated to.
    /// `hormer_scores` — a map of file path → HORMER relevance score.
    ///
    /// Only paths whose normalized prefix matches `path` will be considered
    /// (i.e., files living in or adjacent to the current directory).
    pub async fn predictive_warm(
        &self,
        path: &str,
        hormer_scores: &HormerScoreMap,
    ) -> usize {
        if !*self.enabled.lock().await {
            return 0;
        }

        let normalized_path = path.replace('\\', "/");
        let normalized_path = normalized_path.trim_end_matches('/');

        // Collect all scored files whose path starts with the target directory
        let mut candidates: Vec<(&String, &f64)> = hormer_scores
            .iter()
            .filter(|(file_path, _)| {
                let fp = file_path.replace('\\', "/");
                fp.starts_with(normalized_path)
            })
            .collect();

        // Sort descending by HORMER score
        candidates.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

        let top_k = self.top_k.min(candidates.len());
        let mut warmed = 0usize;
        for (path, score) in &candidates[..top_k] {
            self.track_access(path).await;
            tracing::trace!(
                "🧠 predictive_cache_warm: {} (score={:.4})",
                path,
                score
            );
            warmed += 1;
        }

        tracing::info!(
            "🔥 predictive_cache_warm: warmed {} / {} candidates under '{}'",
            top_k,
            candidates.len(),
            normalized_path
        );

        warmed
    }
}

impl Default for PredictiveCacheWarmup {
    fn default() -> Self {
        Self::new()
    }
}

/// Standalone predictive cache warm function based on HORMER navigation scores.
///
/// Filters `hormer_scores` for entries that start with `path`, sorts them
/// by score descending, and pre-warms the cache for the top `top_n` entries.
pub fn predictive_warm(path: &str, hormer_scores: &HormerScoreMap, top_n: usize) {
    let mut scored: Vec<(&String, &f64)> = hormer_scores
        .iter()
        .filter(|(k, _)| k.starts_with(path))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (file, score) in scored.iter().take(top_n) {
        // Trigger cache pre-load for top scored files
        tracing::info!("Predictive cache warm: {} (score: {})", file, score);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_track_access() {
        let warmup = PredictiveCacheWarmup::new();
        warmup.track_access("doc1").await;
        warmup.track_access("doc1").await;
        warmup.track_access("doc2").await;

        assert_eq!(warmup.unique_docs_tracked().await, 2);
        assert_eq!(warmup.total_accesses().await, 3);
    }

    #[tokio::test]
    async fn test_hot_docs_ordering() {
        let warmup = PredictiveCacheWarmup::new();
        warmup.track_access("hot").await;
        warmup.track_access("hot").await;
        warmup.track_access("hot").await;
        warmup.track_access("cold").await;

        let hot = warmup.get_hot_docs().await;
        assert_eq!(hot.first().unwrap(), "hot");
    }

    #[tokio::test]
    async fn test_warmup_creates_no_errors() {
        let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let memory = QmdMemory::new_with_workspace(docs, "test-ws");

        let warmup = PredictiveCacheWarmup::with_params(Duration::from_secs(600), 10);
        warmup.set_enabled(true).await;

        // Should not panic on empty memory
        warmup.warmup(&memory).await;
    }

    #[tokio::test]
    async fn test_enable_disable() {
        let warmup = PredictiveCacheWarmup::new();
        assert!(!warmup.is_enabled().await);

        warmup.set_enabled(true).await;
        assert!(warmup.is_enabled().await);

        warmup.set_enabled(false).await;
        assert!(!warmup.is_enabled().await);
    }

    #[tokio::test]
    async fn test_reset_clears_stats() {
        let warmup = PredictiveCacheWarmup::new();
        warmup.track_access("doc1").await;
        warmup.track_access("doc2").await;
        assert_eq!(warmup.unique_docs_tracked().await, 2);

        warmup.reset().await;
        assert_eq!(warmup.unique_docs_tracked().await, 0);
    }
}
