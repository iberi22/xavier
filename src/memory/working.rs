//! Working Memory Layer - Bounded FIFO queue with LRU fallback and access tracking
//!
//! Implements the first layer of the Multi-Layer Memory Architecture.
//! Provides fast, bounded storage for recent memory items with:
//! - Bounded capacity (configurable, default 100)
//! - FIFO eviction with LRU fallback for frequently accessed items
//! - Access frequency tracking
//! - Fast in-memory BM25-style search

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// A memory item stored in working memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    /// Unique identifier for this memory item
    pub id: String,
    /// The actual memory content
    pub content: String,
    /// When this item was created
    pub created_at: DateTime<Utc>,
    /// When this item was last accessed
    pub last_accessed: DateTime<Utc>,
    /// Access count for LRU fallback decisions
    pub access_count: u32,
    /// Optional metadata
    pub metadata: Option<serde_json::Value>,
}

impl MemoryItem {
    /// Create a new memory item with current timestamp
    pub fn new(id: impl Into<String>, content: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            content: content.into(),
            created_at: now,
            last_accessed: now,
            access_count: 0,
            metadata: None,
        }
    }

    /// Create a new memory item with metadata
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Update access timestamp and increment access count
    pub fn record_access(&mut self) {
        self.last_accessed = Utc::now();
        self.access_count += 1;
    }
}

/// Scored search result from working memory
#[derive(Debug, Clone)]
pub struct ScoredResult {
    /// The memory item that matched
    pub item: MemoryItem,
    /// Relevance score (BM25-style)
    pub score: f32,
}

/// Default capacity for working memory
pub const DEFAULT_CAPACITY: usize = 100;
/// Default LRU exemption access threshold
pub const DEFAULT_LRU_THRESHOLD: u32 = 2;
/// Default BM25 k1 parameter
pub const DEFAULT_BM25_K1: f32 = 1.5;
/// Default BM25 b parameter
pub const DEFAULT_BM25_B: f32 = 0.75;

/// Working Memory configuration
#[derive(Debug, Clone)]
pub struct WorkingMemoryConfig {
    /// Maximum number of items in working memory
    pub capacity: usize,
    /// Minimum access count to qualify for LRU exemption
    pub lru_exempt_access_threshold: u32,
    /// BM25 k1 parameter
    pub bm25_k1: f32,
    /// BM25 b parameter
    pub bm25_b: f32,
}

impl Default for WorkingMemoryConfig {
    fn default() -> Self {
        Self {
            capacity: DEFAULT_CAPACITY,
            lru_exempt_access_threshold: DEFAULT_LRU_THRESHOLD,
            bm25_k1: DEFAULT_BM25_K1,
            bm25_b: DEFAULT_BM25_B,
        }
    }
}

impl WorkingMemoryConfig {
    /// Load configuration from environment variables
    ///
    /// Reads XAVIER_WORKING_MEMORY_CAPACITY, XAVIER_WORKING_LRU_THRESHOLD,
    /// XAVIER_WORKING_BM25_K1, XAVIER_WORKING_BM25_B with validated defaults.
    pub fn from_env() -> Self {
        let default = Self::default();
        Self {
            capacity: Self::parse_or("XAVIER_WORKING_MEMORY_CAPACITY", default.capacity, |v| {
                *v > 0
            }),
            lru_exempt_access_threshold: Self::parse_or(
                "XAVIER_WORKING_LRU_THRESHOLD",
                default.lru_exempt_access_threshold,
                |v| *v > 0,
            ),
            bm25_k1: Self::parse_or("XAVIER_WORKING_BM25_K1", default.bm25_k1, |v| *v > 0.0),
            bm25_b: Self::parse_or("XAVIER_WORKING_BM25_B", default.bm25_b, |v| *v > 0.0),
        }
    }

    /// Parse an env var with validation; logs warning on invalid input
    fn parse_or<T: std::str::FromStr + std::fmt::Display + PartialOrd>(
        key: &str,
        default: T,
        validate: fn(&T) -> bool,
    ) -> T {
        match std::env::var(key) {
            Ok(val) => match val.parse::<T>() {
                Ok(parsed) if validate(&parsed) => parsed,
                Ok(_) => {
                    tracing::warn!(
                        "{} value '{}' is out of valid range, using default '{}'",
                        key,
                        val,
                        default
                    );
                    default
                }
                Err(_) => {
                    tracing::warn!(
                        "{} value '{}' is not a valid number, using default '{}'",
                        key,
                        val,
                        default
                    );
                    default
                }
            },
            Err(_) => default,
        }
    }
}

impl From<WorkingMemoryConfig> for crate::memory::layers_config::WorkingMemoryLayerConfig {
    fn from(cfg: WorkingMemoryConfig) -> Self {
        Self {
            capacity: cfg.capacity,
            lru_exempt_access_threshold: cfg.lru_exempt_access_threshold,
            bm25_k1: cfg.bm25_k1,
            bm25_b: cfg.bm25_b,
        }
    }
}

/// Working Memory - Bounded FIFO queue with LRU fallback
///
/// # Eviction Strategy
/// 1. Items with access_count < threshold are evicted in FIFO order (oldest first)
/// 2. If no FIFO candidates remain, fall back to LRU (least recently accessed)
/// 3. High-access items are retained longer but still bounded by capacity
///
/// # Example
/// ```rust
/// use xavier::memory::working::{WorkingMemory, MemoryItem};
///
/// let mut wm = WorkingMemory::new();
/// wm.push(MemoryItem::new("1", "First item"));
/// wm.push(MemoryItem::new("2", "Second item"));
///
/// let item = wm.get("1").expect("test assertion");
/// assert_eq!(item.content, "First item");
/// ```
pub struct WorkingMemory {
    config: WorkingMemoryConfig,
    /// FIFO queue of item IDs (oldest first)
    items_queue: VecDeque<String>,
    /// Map from ID to MemoryItem
    items: HashMap<String, MemoryItem>,
    /// Map from ID to position in queue for O(1) lookups
    position_map: HashMap<String, usize>,
    /// Set by every mutation, cleared after a successful snapshot
    dirty: bool,
}

impl WorkingMemory {
    /// Create a new working memory with default config
    pub fn new() -> Self {
        Self::with_config(WorkingMemoryConfig::default())
    }

    /// Create a new working memory with custom config
    pub fn with_config(config: WorkingMemoryConfig) -> Self {
        Self {
            config,
            items_queue: VecDeque::new(),
            items: HashMap::new(),
            position_map: HashMap::new(),
            dirty: false,
        }
    }

    /// Get the current capacity configuration
    pub fn capacity(&self) -> usize {
        self.config.capacity
    }

    /// Get the current number of items
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get an item by ID without recording access
    pub fn get(&self, id: &str) -> Option<&MemoryItem> {
        self.items.get(id)
    }

    /// Get an item by ID and record the access
    pub fn access(&mut self, id: &str) -> Option<&MemoryItem> {
        if let Some(item) = self.items.get_mut(id) {
            item.record_access();
        }
        self.items.get(id)
    }

    /// Push a new item into working memory
    ///
    /// If at capacity, evicts oldest items using FIFO/LRU strategy.
    /// Returns the evicted item if any.
    pub fn push(&mut self, item: MemoryItem) -> Option<MemoryItem> {
        self.dirty = true;
        // If item with same ID exists, update it instead
        if let std::collections::hash_map::Entry::Occupied(mut entry) =
            self.items.entry(item.id.clone())
        {
            let existing = entry.get_mut();
            *existing = item;
            existing.record_access();
            return None;
        }

        let evicted = if self.items.len() >= self.config.capacity {
            self.evict_oldest()
        } else {
            None
        };

        let id = item.id.clone();
        let position = self.items_queue.len();
        self.items_queue.push_back(id.clone());
        self.position_map.insert(id.clone(), position);
        self.items.insert(id, item);

        evicted
    }

    /// Evict the oldest item using FIFO with LRU fallback
    ///
    /// Priority: FIFO for low-access items, LRU for high-access items
    fn evict_oldest(&mut self) -> Option<MemoryItem> {
        // First pass: find oldest item with low access count (FIFO candidates)
        let mut fifo_candidate: Option<String> = None;
        let mut oldest_fifo_time: DateTime<Utc> = Utc::now();

        for id in &self.items_queue {
            if let Some(item) = self.items.get(id) {
                if item.access_count < self.config.lru_exempt_access_threshold
                    && item.created_at < oldest_fifo_time
                {
                    oldest_fifo_time = item.created_at;
                    fifo_candidate = Some(id.clone());
                }
            }
        }

        // If we found a FIFO candidate, evict the oldest one
        if let Some(id_to_evict) = fifo_candidate {
            return self.remove(&id_to_evict);
        }

        // Fallback: evict least recently accessed item (LRU)
        let mut lru_candidate: Option<String> = None;
        let mut oldest_access: DateTime<Utc> = Utc::now();

        for id in &self.items_queue {
            if let Some(item) = self.items.get(id) {
                if item.last_accessed < oldest_access {
                    oldest_access = item.last_accessed;
                    lru_candidate = Some(id.clone());
                }
            }
        }

        if let Some(id) = lru_candidate {
            self.remove(&id)
        } else {
            None
        }
    }

    /// Remove an item by ID
    fn remove(&mut self, id: &str) -> Option<MemoryItem> {
        self.dirty = true;
        // Remove from queue and rebuild position map
        if let Some(pos) = self.position_map.remove(id) {
            self.items_queue.remove(pos);
            // Rebuild position map for remaining items
            self.position_map.clear();
            for (i, queue_id) in self.items_queue.iter().enumerate() {
                self.position_map.insert(queue_id.clone(), i);
            }
        }

        self.items.remove(id)
    }

    /// Remove an item by ID (public API)
    pub fn remove_item(&mut self, id: &str) -> Option<MemoryItem> {
        self.remove(id)
    }

    /// Clear all items
    pub fn clear(&mut self) {
        self.dirty = true;
        self.items.clear();
        self.items_queue.clear();
        self.position_map.clear();
    }

    /// Search working memory using BM25-style scoring
    pub fn search(&self, query: &str, limit: usize) -> Vec<ScoredResult> {
        if query.is_empty() || self.items.is_empty() {
            return Vec::new();
        }

        let query_terms: Vec<&str> = query.split_whitespace().filter(|t| !t.is_empty()).collect();

        if query_terms.is_empty() {
            return Vec::new();
        }

        let avg_doc_len = if self.items.is_empty() {
            1.0
        } else {
            self.items.values().map(|i| i.content.len()).sum::<usize>() as f32
                / self.items.len() as f32
        };

        let mut scored: Vec<ScoredResult> = self
            .items
            .values()
            .map(|item| {
                let score = bm25_score(
                    &item.content,
                    &query_terms,
                    avg_doc_len,
                    self.config.bm25_k1,
                    self.config.bm25_b,
                );
                ScoredResult {
                    item: item.clone(),
                    score,
                }
            })
            .filter(|r| r.score > 0.0)
            .collect();

        // Sort by score descending, then by access count descending (secondary)
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.item.access_count.cmp(&a.item.access_count))
        });

        scored.truncate(limit);
        scored
    }

    /// Get all items as a vector (ordered by insertion time, oldest first)
    pub fn items(&self) -> Vec<&MemoryItem> {
        self.items_queue
            .iter()
            .filter_map(|id| self.items.get(id))
            .collect()
    }

    /// Get statistics about working memory
    pub fn stats(&self) -> WorkingMemoryStats {
        let total_accesses: u32 = self.items.values().map(|i| i.access_count).sum();
        let avg_access = if self.items.is_empty() {
            0.0
        } else {
            total_accesses as f32 / self.items.len() as f32
        };

        WorkingMemoryStats {
            item_count: self.items.len(),
            capacity: self.config.capacity,
            total_accesses,
            average_access_count: avg_access,
            eviction_candidates: self
                .items
                .values()
                .filter(|i| i.access_count < self.config.lru_exempt_access_threshold)
                .count(),
        }
    }
}

/// On-disk snapshot format version
const SNAPSHOT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct WorkingMemorySnapshot {
    version: u32,
    /// Items in insertion order (oldest first)
    items: Vec<MemoryItem>,
}

/// Default snapshot cadence in seconds
pub const DEFAULT_SNAPSHOT_INTERVAL_SECS: u64 = 60;

/// Canonical snapshot path under a state directory
pub fn snapshot_path(state_dir: &std::path::Path) -> std::path::PathBuf {
    state_dir.join("working_memory").join("snapshot.json")
}

/// Snapshot cadence from `XAVIER_WORKING_SNAPSHOT_INTERVAL_SECS` (0 disables).
pub fn snapshot_interval_from_env() -> Option<std::time::Duration> {
    let secs = match std::env::var("XAVIER_WORKING_SNAPSHOT_INTERVAL_SECS") {
        Ok(v) => v.trim().parse::<u64>().unwrap_or_else(|_| {
            tracing::warn!(
                "XAVIER_WORKING_SNAPSHOT_INTERVAL_SECS '{}' is invalid, using default",
                v
            );
            DEFAULT_SNAPSHOT_INTERVAL_SECS
        }),
        Err(_) => DEFAULT_SNAPSHOT_INTERVAL_SECS,
    };
    (secs > 0).then(|| std::time::Duration::from_secs(secs))
}

impl WorkingMemory {
    /// True when state changed since the last successful snapshot
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Atomically write a snapshot (temp file + rename) and clear the dirty flag.
    pub fn save_snapshot(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        let snapshot = WorkingMemorySnapshot {
            version: SNAPSHOT_VERSION,
            items: self.items().into_iter().cloned().collect(),
        };
        let bytes = serde_json::to_vec(&snapshot).map_err(std::io::Error::other)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        if let Some(parent) = path.parent() {
            // Make the rename itself durable (best effort; unsupported on some platforms).
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        self.dirty = false;
        Ok(())
    }

    /// Restore items from a snapshot, merging into the current contents.
    ///
    /// A missing file is a normal first boot. A corrupt or unreadable snapshot
    /// is logged and ignored: boot must never fail because of it. Capacity is
    /// always respected (older items are evicted first). Returns items restored.
    pub fn restore_snapshot(&mut self, path: &std::path::Path) -> usize {
        // A crash mid-save can leave a stale temp file behind.
        let _ = std::fs::remove_file(path.with_extension("json.tmp"));
        let raw = match std::fs::read(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return 0,
            Err(e) => {
                tracing::error!(path = %path.display(), error = %e, "working memory snapshot unreadable; starting empty");
                return 0;
            }
        };
        let snapshot = match serde_json::from_slice::<WorkingMemorySnapshot>(&raw) {
            Ok(s) if s.version == SNAPSHOT_VERSION => s,
            Ok(s) => {
                tracing::error!(path = %path.display(), version = s.version, "working memory snapshot has unknown version; starting empty");
                return 0;
            }
            Err(e) => {
                tracing::error!(path = %path.display(), error = %e, "working memory snapshot corrupt; starting empty");
                return 0;
            }
        };
        // Keep only the newest `capacity` items so restore never churns evictions.
        let skip = snapshot.items.len().saturating_sub(self.config.capacity);
        let mut restored = 0;
        for item in snapshot.items.into_iter().skip(skip) {
            if self.items.contains_key(&item.id) {
                continue;
            }
            self.push(item);
            restored += 1;
        }
        self.dirty = false;
        restored
    }
}

/// Save the snapshot if dirty (blocking file IO runs off the async workers).
pub async fn save_if_dirty(
    wm: &std::sync::Arc<tokio::sync::RwLock<WorkingMemory>>,
    path: &std::path::Path,
) {
    let wm = std::sync::Arc::clone(wm);
    let path = path.to_path_buf();
    let result = tokio::task::spawn_blocking(move || {
        let mut guard = wm.blocking_write();
        if guard.is_dirty() {
            guard.save_snapshot(&path)
        } else {
            Ok(())
        }
    })
    .await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(error = %e, "working memory snapshot failed"),
        Err(e) => tracing::warn!(error = %e, "working memory snapshot task failed"),
    }
}

/// Restore the snapshot, then snapshot periodically (only when dirty) and on SIGTERM.
///
/// The HTTP server's own graceful shutdown (SIGINT) additionally calls
/// [`save_if_dirty`] directly.
pub fn spawn_persistence(
    wm: std::sync::Arc<tokio::sync::RwLock<WorkingMemory>>,
    path: std::path::PathBuf,
    interval: Option<std::time::Duration>,
) {
    {
        let wm = std::sync::Arc::clone(&wm);
        let path = path.clone();
        tokio::spawn(async move {
            let restored = wm.write().await.restore_snapshot(&path);
            if restored > 0 {
                tracing::info!(restored, "working memory restored from snapshot");
            }
        });
    }
    let Some(interval) = interval else {
        tracing::info!("working memory periodic snapshot disabled");
        return;
    };
    {
        let wm = std::sync::Arc::clone(&wm);
        let path = path.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                save_if_dirty(&wm, &path).await;
            }
        });
    }
    #[cfg(unix)]
    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
            sigterm.recv().await;
            save_if_dirty(&wm, &path).await;
        }
    });
}

impl Default for WorkingMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// BM25 scoring function for keyword matching
fn bm25_score(doc_content: &str, query_terms: &[&str], avg_doc_len: f32, k1: f32, b: f32) -> f32 {
    let doc_lower = doc_content.to_lowercase();
    let doc_len = doc_content.len() as f32;

    let mut score = 0.0f32;

    for term in query_terms {
        let term_lower = term.to_lowercase();
        let term_count = doc_lower.matches(&term_lower).count() as f32;

        if term_count > 0.0 {
            // Term frequency component (simplified BM25)
            let tf =
                (k1 + 1.0) * term_count / (k1 * (1.0 - b + b * doc_len / avg_doc_len) + term_count);

            // For simplicity, IDF is constant; in production, compute document frequency
            let idf = 1.0;

            score += tf * idf;
        }
    }

    score
}

/// Working memory statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingMemoryStats {
    pub item_count: usize,
    pub capacity: usize,
    pub total_accesses: u32,
    pub average_access_count: f32,
    pub eviction_candidates: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_get() {
        let mut wm = WorkingMemory::new();
        wm.push(MemoryItem::new("1", "Hello world"));
        wm.push(MemoryItem::new("2", "Test content"));

        assert_eq!(wm.get("1").expect("test assertion").content, "Hello world");
        assert_eq!(wm.get("2").expect("test assertion").content, "Test content");
        assert!(wm.get("3").is_none());
    }

    #[test]
    fn test_access_tracking() {
        let mut wm = WorkingMemory::new();
        wm.push(MemoryItem::new("1", "Test item"));

        assert_eq!(wm.get("1").expect("test assertion").access_count, 0);

        wm.access("1");
        assert_eq!(wm.get("1").expect("test assertion").access_count, 1);

        wm.access("1");
        wm.access("1");
        assert_eq!(wm.get("1").expect("test assertion").access_count, 3);
    }

    #[test]
    fn test_fifo_eviction() {
        let mut wm = WorkingMemory::with_config(WorkingMemoryConfig {
            capacity: 3,
            lru_exempt_access_threshold: 2,
            bm25_k1: 1.5,
            bm25_b: 0.75,
        });

        wm.push(MemoryItem::new("1", "First"));
        wm.push(MemoryItem::new("2", "Second"));
        wm.push(MemoryItem::new("3", "Third"));

        // Adding 4th item should evict "First" (oldest with low access)
        let evicted = wm.push(MemoryItem::new("4", "Fourth"));
        assert!(evicted.is_some());
        assert_eq!(evicted.expect("test assertion").id, "1");

        assert!(wm.get("1").is_none());
        assert!(wm.get("2").is_some());
        assert!(wm.get("3").is_some());
        assert!(wm.get("4").is_some());
    }

    #[test]
    fn test_lru_fallback() {
        let mut wm = WorkingMemory::with_config(WorkingMemoryConfig {
            capacity: 2,
            lru_exempt_access_threshold: 2,
            bm25_k1: 1.5,
            bm25_b: 0.75,
        });

        wm.push(MemoryItem::new("1", "First"));
        wm.push(MemoryItem::new("2", "Second"));

        // Access item "1" multiple times to make it high-access
        wm.access("1");
        wm.access("1");
        wm.access("1");

        // Now add item "3" - should evict "2" (oldest with low access, even though "1" is older)
        let evicted = wm.push(MemoryItem::new("3", "Third"));

        assert!(evicted.is_some());
        assert_eq!(evicted.expect("test assertion").id, "2");
        assert!(wm.get("1").is_some()); // High-access item retained
        assert!(wm.get("3").is_some());
    }

    #[test]
    fn test_search() {
        let mut wm = WorkingMemory::new();
        wm.push(MemoryItem::new("1", "Rust programming language"));
        wm.push(MemoryItem::new("2", "Python for data science"));
        wm.push(MemoryItem::new("3", "Rust async programming"));

        let results = wm.search("Rust", 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].item.id, "3");
        assert_eq!(results[1].item.id, "1");

        // The Python query only matches the Python document.
        let python_results = wm.search("Python", 10);
        assert_eq!(python_results.len(), 1);
        assert_eq!(python_results[0].item.id, "2");
    }

    #[test]
    fn test_update_existing() {
        let mut wm = WorkingMemory::new();
        wm.push(MemoryItem::new("1", "Original content"));

        // Push same ID should update, not add
        wm.push(MemoryItem::new("1", "Updated content"));

        assert_eq!(wm.len(), 1);
        assert_eq!(
            wm.get("1").expect("test assertion").content,
            "Updated content"
        );
    }

    #[test]
    fn test_remove() {
        let mut wm = WorkingMemory::new();
        wm.push(MemoryItem::new("1", "Test"));
        wm.push(MemoryItem::new("2", "Test 2"));

        let removed = wm.remove_item("1");
        assert!(removed.is_some());
        assert_eq!(removed.expect("test assertion").id, "1");
        assert!(wm.get("1").is_none());
        assert_eq!(wm.len(), 1);
    }

    #[test]
    fn test_clear() {
        let mut wm = WorkingMemory::new();
        wm.push(MemoryItem::new("1", "Test"));
        wm.push(MemoryItem::new("2", "Test 2"));

        wm.clear();
        assert!(wm.is_empty());
        assert_eq!(wm.len(), 0);
    }

    #[test]
    fn test_stats() {
        let mut wm = WorkingMemory::new();
        wm.push(MemoryItem::new("1", "Test"));
        wm.push(MemoryItem::new("2", "Test 2"));
        wm.access("1");
        wm.access("1");

        let stats = wm.stats();
        assert_eq!(stats.item_count, 2);
        assert_eq!(stats.capacity, 100);
        assert_eq!(stats.total_accesses, 2);
        assert!((stats.average_access_count - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_bm25_scoring() {
        let doc = "The quick brown fox jumps over the lazy dog";
        let query = vec!["quick", "fox"];
        let avg_len = 10.0;

        let score = bm25_score(doc, &query, avg_len, 1.5, 0.75);
        assert!(score > 0.0);

        // Non-matching query should have 0 score
        let no_match = bm25_score(doc, &["xyz"], avg_len, 1.5, 0.75);
        assert_eq!(no_match, 0.0);
    }

    #[test]
    fn test_snapshot_roundtrip_and_dirty_flag() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = snapshot_path(dir.path());
        let mut wm = WorkingMemory::new();
        assert!(!wm.is_dirty());
        wm.push(MemoryItem::new("1", "alpha"));
        wm.push(MemoryItem::new("2", "beta"));
        assert!(wm.is_dirty());
        wm.save_snapshot(&path).expect("save");
        assert!(!wm.is_dirty());
        assert!(!path.with_extension("json.tmp").exists());

        let mut fresh = WorkingMemory::new();
        assert_eq!(fresh.restore_snapshot(&path), 2);
        assert_eq!(fresh.get("2").expect("item").content, "beta");
        let order: Vec<&str> = fresh.items().iter().map(|i| i.id.as_str()).collect();
        assert_eq!(order, ["1", "2"]);
        assert!(!fresh.is_dirty());
    }

    #[test]
    fn test_snapshot_restore_respects_capacity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = snapshot_path(dir.path());
        let mut wm = WorkingMemory::new();
        for i in 0..10 {
            wm.push(MemoryItem::new(i.to_string(), "x"));
        }
        wm.save_snapshot(&path).expect("save");

        let mut small = WorkingMemory::with_config(WorkingMemoryConfig {
            capacity: 3,
            ..WorkingMemoryConfig::default()
        });
        assert_eq!(small.restore_snapshot(&path), 3);
        assert_eq!(small.len(), 3);
        assert!(small.get("9").is_some());
    }

    #[test]
    fn test_corrupt_or_missing_snapshot_starts_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = snapshot_path(dir.path());
        let mut wm = WorkingMemory::new();
        assert_eq!(wm.restore_snapshot(&path), 0);

        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, b"{ not json").expect("write");
        assert_eq!(wm.restore_snapshot(&path), 0);
        assert!(wm.is_empty());
    }
}
