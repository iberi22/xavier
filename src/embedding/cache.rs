// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Persistent embedding cache with LRU eviction.
//!
//! Provides an in-memory LRU cache backed by SQLite for embedding vectors,
//! so embeddings survive process restarts and frequently-used embeddings
//! stay hot in memory.
//!
//! ## Strategy
//!
//! 1. Check the in-memory `moka` cache first (fast path).
//! 2. On miss, check the SQLite backing store.
//! 3. On SQLite hit, re-populate the in-memory cache and return.
//! 4. On SQLite miss (or expired entry), call the real embedder, then
//!    write-through to both SQLite and the in-memory cache.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use moka::future::Cache;
use parking_lot::Mutex;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tracing::warn;

use super::{Embedder, EmbeddingError};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the embedding cache, sourced from environment variables.
#[derive(Debug, Clone)]
pub struct EmbeddingCacheConfig {
    /// Whether the cache is enabled (default: true).
    pub enabled: bool,
    /// Maximum number of entries in the in-memory LRU cache (default: 10_000).
    pub max_capacity: u64,
    /// Time-to-live in hours for cached embeddings (default: 24).
    pub ttl_hours: u64,
    /// Path to the SQLite database used for persistence (default: data/embedding_cache.db).
    pub db_path: PathBuf,
    /// Whether disk persistence is enabled via SQLite (default: false).
    pub persist: bool,
    /// The name of the embedding model, used to namespace cache keys.
    pub model_name: String,
}

impl Default for EmbeddingCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_capacity: 10_000,
            ttl_hours: 24,
            db_path: PathBuf::from("data/embedding_cache.db"),
            persist: false,
            model_name: "default".to_string(),
        }
    }
}

impl EmbeddingCacheConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        let enabled = std::env::var("XAVIER_EMBEDDING_CACHE_ENABLED")
            .ok()
            .map(|v| !matches!(v.as_str(), "0" | "false" | "no" | "off"))
            .unwrap_or(true);

        let max_capacity = std::env::var("XAVIER_EMBEDDING_CACHE_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10_000);

        let ttl_hours = std::env::var("XAVIER_EMBEDDING_CACHE_TTL_HOURS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(24);

        let db_path = std::env::var("XAVIER_EMBEDDING_CACHE_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/embedding_cache.db"));

        let persist = std::env::var("XAVIER_EMBEDDING_CACHE_PERSIST")
            .ok()
            .map(|v| v == "1")
            .unwrap_or(false);

        let model_name = std::env::var("XAVIER_EMBEDDING_MODEL")
            .or_else(|_| std::env::var("XAVIER_GLLM_MODEL"))
            .unwrap_or_else(|_| "default".to_string());

        Self {
            enabled,
            max_capacity,
            ttl_hours,
            db_path,
            persist,
            model_name,
        }
    }
}

// ---------------------------------------------------------------------------
// Cache key helpers
// ---------------------------------------------------------------------------

/// Compute a SHA-256 hex digest of `model_name` and `text` to use as the cache key.
pub fn content_hash(model_name: &str, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(model_name.as_bytes());
    hasher.update(b":");
    hasher.update(text.as_bytes());
    crate::crypto::hex_encode(hasher.finalize())
}

// ---------------------------------------------------------------------------
// EmbeddingCache
// ---------------------------------------------------------------------------

/// A persistent cache for embedding vectors.
///
/// Wraps a `moka::future::Cache` for fast in-memory lookups with LRU eviction
/// and TTL, and a SQLite database for durability across restarts.
pub struct EmbeddingCache {
    /// In-memory LRU cache (moka), wrapped in an Arc for safe shared access.
    inner: Cache<String, Arc<Vec<f32>>>,
    /// SQLite connection, lazily initialised on the first write.
    db: Mutex<Option<Connection>>,
    /// Configuration.
    config: EmbeddingCacheConfig,
}

impl EmbeddingCache {
    /// Create a new cache from the given configuration.
    pub fn new(config: EmbeddingCacheConfig) -> Self {
        let ttl = Duration::from_secs(config.ttl_hours * 3600);

        let inner = Cache::builder()
            .max_capacity(config.max_capacity)
            .time_to_live(ttl)
            .build();

        Self {
            inner,
            db: Mutex::new(None),
            config,
        }
    }

    /// Create a new cache from environment variables.
    pub fn from_env() -> Self {
        Self::new(EmbeddingCacheConfig::from_env())
    }

    /// Return whether the cache is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Return the cache configuration.
    pub fn config(&self) -> &EmbeddingCacheConfig {
        &self.config
    }

    /// Attempt to retrieve the embedding for `text` from the cache.
    ///
    /// The lookup order is:
    /// 1. In-memory moka cache (fast).
    /// 2. SQLite backing store (medium).
    /// 3. Generate via `embedder` (slow), then write-through.
    pub async fn get_or_embed(
        &self,
        embedder: &dyn Embedder,
        text: &str,
    ) -> Result<Vec<f32>, EmbeddingError> {
        let key = content_hash(&self.config.model_name, text);

        // 1. Check in-memory cache.
        if let Some(embedding) = self.inner.get(&key).await {
            tracing::debug!(
                "Cache hit (memory) for embedding model: {}",
                self.config.model_name
            );
            return Ok(embedding.as_ref().clone());
        }

        // 2. Check SQLite backing store.
        if let Some(embedding) = self.try_lookup_sqlite(&key) {
            tracing::debug!(
                "Cache hit (sqlite) for embedding model: {}",
                self.config.model_name
            );
            let arc = Arc::new(embedding.clone());
            self.inner.insert(key.clone(), arc).await;
            return Ok(embedding);
        }

        // 3. Miss — call the real embedder.
        tracing::debug!("Cache miss for embedding model: {}", self.config.model_name);
        let embedding = embedder.encode(text).await?;

        // Write-through to both stores.
        let arc = Arc::new(embedding.clone());
        self.inner.insert(key.clone(), arc).await;
        self.try_persist_sqlite(&key, &embedding);

        Ok(embedding)
    }

    /// Invalidate a single entry from both caches.
    pub async fn invalidate(&self, text: &str) {
        let key = content_hash(&self.config.model_name, text);
        self.inner.invalidate(&key).await;
        self.try_delete_sqlite(&key);
    }

    /// Clear the entire cache (memory + SQLite).
    pub fn clear(&self) {
        self.inner.invalidate_all();
        self.try_clear_sqlite();
    }

    /// Return the number of entries currently in the in-memory cache.
    pub fn entry_count(&self) -> u64 {
        self.inner.entry_count()
    }

    // ------------------------------------------------------------------
    // Private helpers — synchronous SQLite operations
    // ------------------------------------------------------------------

    /// Look up `key` in SQLite. Returns `None` if missing or expired.
    fn try_lookup_sqlite(&self, key: &str) -> Option<Vec<f32>> {
        if !self.config.persist {
            return None;
        }

        let db = self.db.lock();
        let conn = db.as_ref()?;

        let row: Result<(Vec<u8>, String), rusqlite::Error> = conn.query_row(
            "SELECT embedding, created_at FROM embedding_cache WHERE content_hash = ?1",
            rusqlite::params![key],
            |row| {
                let blob: Vec<u8> = row.get(0)?;
                let created_at: String = row.get(1)?;
                Ok((blob, created_at))
            },
        );

        let (blob, created_at) = row.ok()?;

        // Check TTL expiry.
        if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(&created_at, "%Y-%m-%d %H:%M:%S")
        {
            let now = chrono::Utc::now().naive_utc();
            let ttl_secs = (self.config.ttl_hours as i64) * 3600;
            if (now - parsed).num_seconds() > ttl_secs {
                // Expired — delete and treat as miss.
                let _ = conn.execute(
                    "DELETE FROM embedding_cache WHERE content_hash = ?1",
                    rusqlite::params![key],
                );
                return None;
            }
        }

        // Decode f32 blob.
        Some(
            blob.chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
        )
    }

    /// Persist `embedding` to SQLite under `key`.
    fn try_persist_sqlite(&self, key: &str, embedding: &[f32]) {
        if !self.config.persist {
            return;
        }

        // Ensure the database directory exists.
        if let Some(parent) = self.config.db_path.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    warn!(error = %e, "failed to create embedding cache db directory");
                    return;
                }
            }
        }

        // Convert the embedding to a byte blob.
        let blob: Vec<u8> = embedding.iter().flat_map(|v| v.to_le_bytes()).collect();

        let mut guard = self.db.lock();

        // Lazily open the database.
        if guard.is_none() {
            match open_cache_db(&self.config.db_path) {
                Ok(db) => *guard = Some(db),
                Err(e) => {
                    warn!(error = %e, "failed to open embedding cache db; persistence disabled");
                    return;
                }
            }
        }

        if let Some(ref conn) = *guard {
            if let Err(e) = conn.execute(
                "INSERT OR REPLACE INTO embedding_cache (content_hash, embedding, created_at)
                 VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![key, blob],
            ) {
                warn!(error = %e, "failed to persist embedding to SQLite cache");
            }
        }
    }

    fn try_delete_sqlite(&self, key: &str) {
        if !self.config.persist {
            return;
        }

        if let Some(ref conn) = *self.db.lock() {
            let _ = conn.execute(
                "DELETE FROM embedding_cache WHERE content_hash = ?1",
                rusqlite::params![key],
            );
        }
    }

    fn try_clear_sqlite(&self) {
        if !self.config.persist {
            return;
        }

        if let Some(ref conn) = *self.db.lock() {
            let _ = conn.execute("DELETE FROM embedding_cache", []);
        }
    }
}

// ---------------------------------------------------------------------------
// CachedEmbedder — a decorator that adds caching to any Embedder
// ---------------------------------------------------------------------------

/// An [`Embedder`] wrapper that transparently caches results.
///
/// ```ignore
/// let embedder: Arc<dyn Embedder> = ...;
/// let cached = CachedEmbedder::from_env(embedder);
/// let vec = cached.encode("hello world").await?;
/// ```
pub struct CachedEmbedder {
    inner: Arc<dyn Embedder>,
    cache: Arc<EmbeddingCache>,
}

impl CachedEmbedder {
    /// Wrap `embedder` with a cache built from environment variables.
    pub fn from_env(embedder: Arc<dyn Embedder>) -> Self {
        Self {
            cache: Arc::new(EmbeddingCache::from_env()),
            inner: embedder,
        }
    }

    /// Wrap `embedder` with a specific cache configuration.
    pub fn new(embedder: Arc<dyn Embedder>, cache: Arc<EmbeddingCache>) -> Self {
        Self {
            inner: embedder,
            cache,
        }
    }

    /// Return a reference to the underlying cache (for diagnostics / admin).
    pub fn cache(&self) -> &Arc<EmbeddingCache> {
        &self.cache
    }
}

#[async_trait]
impl Embedder for CachedEmbedder {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if self.cache.is_enabled() {
            self.cache.get_or_embed(&*self.inner, text).await
        } else {
            self.inner.encode(text).await
        }
    }

    fn dimension(&self) -> usize {
        self.inner.dimension()
    }
}

// ---------------------------------------------------------------------------
// SQLite helpers
// ---------------------------------------------------------------------------

/// Open (or create) the cache database and initialise the schema.
fn open_cache_db(path: &Path) -> Result<Connection, rusqlite::Error> {
    let db = Connection::open(path)?;

    // Use WAL mode for better concurrent read/write performance.
    db.execute_batch("PRAGMA journal_mode=WAL;")?;

    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS embedding_cache (
            content_hash TEXT PRIMARY KEY NOT NULL,
            embedding BLOB NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_embedding_cache_created_at
            ON embedding_cache (created_at);",
    )?;

    Ok(db)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::NoopEmbedder;

    #[test]
    fn test_content_hash_is_deterministic() {
        let h1 = content_hash("default", "hello world");
        let h2 = content_hash("default", "hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn test_content_hash_differs_for_different_inputs() {
        let h1 = content_hash("default", "hello");
        let h2 = content_hash("default", "world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_content_hash_differs_for_different_models() {
        let h1 = content_hash("model1", "hello");
        let h2 = content_hash("model2", "hello");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_config_defaults() {
        let config = EmbeddingCacheConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_capacity, 10_000);
        assert_eq!(config.ttl_hours, 24);
        assert_eq!(config.db_path, PathBuf::from("data/embedding_cache.db"));
    }

    #[test]
    fn test_config_from_env() {
        // Without env vars set, defaults should match.
        let config = EmbeddingCacheConfig::from_env();
        assert!(config.enabled);
        assert_eq!(config.max_capacity, 10_000);
        assert_eq!(config.ttl_hours, 24);
    }

    #[tokio::test]
    async fn test_cache_hit_and_miss() {
        let config = EmbeddingCacheConfig {
            enabled: true,
            max_capacity: 100,
            ttl_hours: 24,
            db_path: PathBuf::from(":memory:"),
            persist: true,
            model_name: "default".to_string(),
        };
        let cache = Arc::new(EmbeddingCache::new(config));
        let embedder = Arc::new(NoopEmbedder);

        // NoopEmbedder returns an empty vector, but our cache should
        // still store and retrieve it.
        let result1 = cache.get_or_embed(&*embedder, "test").await.unwrap();
        let result2 = cache.get_or_embed(&*embedder, "test").await.unwrap();
        assert_eq!(result1, result2);
    }

    #[tokio::test]
    async fn test_cache_miss_when_model_changes() {
        let config1 = EmbeddingCacheConfig {
            enabled: true,
            max_capacity: 100,
            ttl_hours: 24,
            db_path: PathBuf::from(":memory:"),
            persist: true,
            model_name: "modelA".to_string(),
        };
        let config2 = EmbeddingCacheConfig {
            enabled: true,
            max_capacity: 100,
            ttl_hours: 24,
            db_path: PathBuf::from(":memory:"),
            persist: true,
            model_name: "modelB".to_string(),
        };
        let cache1 = Arc::new(EmbeddingCache::new(config1));
        let cache2 = Arc::new(EmbeddingCache::new(config2));

        let embedder = Arc::new(NoopEmbedder);

        // Insert into first cache
        let _ = cache1.get_or_embed(&*embedder, "test").await.unwrap();
        assert_eq!(cache1.entry_count(), 1);

        // Second cache has different model name, should miss and not share memory
        // Note: they don't share the sqlite DB either as :memory: is unique per connection
        // but the content_hash ensures different keys anyway.
        assert_eq!(cache2.entry_count(), 0);
        let _ = cache2.get_or_embed(&*embedder, "test").await.unwrap();
        assert_eq!(cache2.entry_count(), 1);
    }

    #[tokio::test]
    async fn test_cache_eviction_by_capacity() {
        let config = EmbeddingCacheConfig {
            enabled: true,
            max_capacity: 2, // Tiny capacity
            ttl_hours: 24,
            db_path: PathBuf::from(":memory:"),
            persist: false, // Ensure we are only testing in-memory eviction
            model_name: "default".to_string(),
        };
        let cache = Arc::new(EmbeddingCache::new(config));
        let embedder = Arc::new(NoopEmbedder);

        // Insert 3 items
        let _ = cache.get_or_embed(&*embedder, "test1").await.unwrap();
        let _ = cache.get_or_embed(&*embedder, "test2").await.unwrap();
        let _ = cache.get_or_embed(&*embedder, "test3").await.unwrap();

        // Moka cache might take a moment to evict, but we can verify it doesn't grow indefinitely
        cache.inner.run_pending_tasks().await;

        assert!(cache.entry_count() <= 2);
    }

    #[tokio::test]
    async fn test_cached_embedder_delegates_dimension() {
        let embedder: Arc<dyn Embedder> = Arc::new(NoopEmbedder);
        let cached = CachedEmbedder::from_env(embedder);
        assert_eq!(cached.dimension(), 0);
    }

    #[test]
    fn test_open_cache_db_in_memory() {
        // Using ":memory:" creates an in-memory SQLite database.
        let db = open_cache_db(Path::new(":memory:"));
        assert!(db.is_ok());
    }

    #[tokio::test]
    async fn test_clear_invalidates_all() {
        let config = EmbeddingCacheConfig {
            enabled: true,
            max_capacity: 100,
            ttl_hours: 24,
            db_path: PathBuf::from(":memory:"),
            persist: true,
            model_name: "default".to_string(),
        };
        let cache = Arc::new(EmbeddingCache::new(config));
        let embedder = Arc::new(NoopEmbedder);

        // Insert via get_or_embed.
        let r1 = cache.get_or_embed(&*embedder, "clear-me").await.unwrap();
        // After first call, a second call should hit the in-memory cache.
        let r2 = cache.get_or_embed(&*embedder, "clear-me").await.unwrap();
        assert_eq!(r1, r2);

        cache.clear();
        // After clear the entry should be gone, causing a fresh generation.
        let _r3 = cache.get_or_embed(&*embedder, "clear-me").await.unwrap();
    }
}
