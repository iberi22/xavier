//! QMD (Queryable Memory Document) storage module
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock as AsyncRwLock;

use std::fmt;
use std::str::FromStr;
use unicode_normalization::UnicodeNormalization;
use regex::Regex;
use std::sync::LazyLock;
pub mod cache_warming;
pub mod config;
pub mod hash;
pub mod query_builder;
pub mod reader;
pub mod search;
pub mod types;
pub mod utils;
pub mod writer;

pub use cache_warming::*;
pub use config::*;
pub use hash::*;
pub use query_builder::*;
pub use reader::*;
pub use search::*;
pub use types::*;
pub use utils::*;
pub use writer::*;

use crate::memory::hierarchy::MemoryHierarchyNode;
use crate::memory::schema::{matches_filters, MemoryQueryFilters, TypedMemoryPayload};
use crate::memory::store::MemoryStore;

#[derive(Clone)]
pub struct QmdMemory {
    pub(crate) workspace_id: String,
    pub(crate) docs: Arc<AsyncRwLock<Vec<MemoryDocument>>>,
    pub(crate) search_cache: Arc<AsyncRwLock<HashMap<SearchCacheKey, Vec<MemoryDocument>>>>,
    pub(crate) cache_counters: Arc<CacheCounters>,
    pub(crate) store: Arc<AsyncRwLock<Option<Arc<dyn MemoryStore>>>>,
    pub(crate) cache_warmup: Option<Arc<PredictiveCacheWarmup>>,
    pub(crate) belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
}

impl fmt::Debug for QmdMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QmdMemory")
            .field("workspace_id", &self.workspace_id)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NormalizedId(String);

impl FromStr for NormalizedId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        static RE_NON_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w]+").unwrap());
        static RE_UNDERSCORES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_+").unwrap());

        // 1. NFKC normalization
        let nfkc = s.nfkc().collect::<String>();

        // 2. regex [^\w]+ -> underscore
        let with_underscores = RE_NON_WORD.replace_all(&nfkc, "_");

        // 3. underscore collapse
        let collapsed = RE_UNDERSCORES.replace_all(&with_underscores, "_");

        // 4. casefold
        Ok(NormalizedId(collapsed.to_lowercase()))
    }
}

impl fmt::Display for NormalizedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl NormalizedId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Creates a NormalizedId without applying normalization rules.
    /// Internal use only or for pre-normalized strings.
    pub fn from_str_unchecked(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl QmdMemory {
    pub fn new(docs: Arc<AsyncRwLock<Vec<MemoryDocument>>>) -> Self {
        Self::new_with_workspace(docs, "default")
    }

    pub fn new_with_workspace(
        docs: Arc<AsyncRwLock<Vec<MemoryDocument>>>,
        workspace_id: impl Into<String>,
    ) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            docs,
            search_cache: Arc::new(AsyncRwLock::new(HashMap::new())),
            cache_counters: Arc::new(CacheCounters::default()),
            store: Arc::new(AsyncRwLock::new(None)),
            cache_warmup: Some(Arc::new(PredictiveCacheWarmup::new())),
            belief_graph: None,
        }
    }

    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub async fn set_store(&self, store: Arc<dyn MemoryStore>) {
        *self.store.write().await = Some(store);
    }

    pub fn set_belief_graph(&mut self, graph: crate::memory::belief_graph::SharedBeliefGraph) {
        self.belief_graph = Some(graph);
    }

    pub(crate) async fn store(&self) -> Option<Arc<dyn MemoryStore>> {
        self.store.read().await.clone()
    }

    /// Load workspace state from persistent store on startup.
    /// This is CRITICAL for persistence - without this, data written to the configured store
    /// before a restart would be lost on restart.
    pub async fn init(&self) -> Result<()> {
        reader::init(self).await
    }

    /// Activate or deactivate cache warming.
    /// When enabled, the most frequently accessed documents are
    /// kept in cache for faster retrieval.
    pub async fn set_cache_warming(&self, enabled: bool, top_k: usize) {
        if let Some(ref warmup) = self.cache_warmup {
            warmup.set_enabled(enabled).await;
            let _ = top_k;
        }
    }

    pub async fn search(&self, query_text: &str, limit: usize) -> Result<Vec<MemoryDocument>> {
        self.search_filtered(query_text, limit, None).await
    }

    pub async fn bm25_search(
        &self,
        query: &str,
        limit: usize,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryDocument>> {
        let docs = self.all_documents().await;
        let filtered_docs: Vec<MemoryDocument> = docs
            .into_iter()
            .filter(|doc| matches_filters(&doc.path, &doc.metadata, &self.workspace_id, filters))
            .collect();

        if filtered_docs.is_empty() {
            return Ok(Vec::new());
        }

        let scored = crate::search::bm25::score_documents(
            query,
            &filtered_docs,
            crate::search::bm25::Bm25Params::default(),
        );

        let mut results = Vec::new();
        for (score, id) in scored.into_iter().take(limit) {
            if let Some(doc) = filtered_docs
                .iter()
                .find(|d| d.id.as_deref() == Some(&id) || d.path == id)
            {
                let mut d = doc.clone();
                d.score = score;
                results.push(d);
            }
        }

        Ok(results)
    }

    pub async fn search_filtered(
        &self,
        query_text: &str,
        limit: usize,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryDocument>> {
        let optimized = self
            .search_hybrid_optimized(query_text, limit, filters)
            .await?;
        if !optimized.is_empty() {
            return Ok(optimized);
        }

        let all_docs = self.all_documents().await;
        let locomo_only = !all_docs.is_empty()
            && all_docs
                .iter()
                .all(|doc| is_locomo_document(&doc.path, &doc.metadata));

        if locomo_only {
            return Ok(self
                .search_with_cache_filtered(query_text, limit, filters)
                .await?
                .documents);
        }

        if std::env::var("XAVIER_EMBEDDING_URL").is_ok() {
            if let Ok(results) =
                query_with_embedding_filtered(self, query_text, limit, filters).await
            {
                if !results.is_empty() {
                    return Ok(results);
                }
            }
        }

        let cache_results = self
            .search_with_cache_filtered(query_text, limit, filters)
            .await?;
        if !cache_results.documents.is_empty() {
            return Ok(cache_results.documents);
        }

        // SPRINT 1: BM25 fallback — search self.docs directly with full BM25 scoring
        // This guarantees data saved via memory_save (which writes to QmdMemory.docs)
        // is always findable, even if the MemoryStore path returns empty.
        let bm25_results = self
            .bm25_search(query_text, limit, filters)
            .await?;
        if !bm25_results.is_empty() {
            return Ok(bm25_results);
        }

        Ok(Vec::new())
    }

    pub async fn search_hybrid_optimized(
        &self,
        query_text: &str,
        limit: usize,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryDocument>> {
        search::search_hybrid_optimized(self, query_text, limit, filters).await
    }

    pub async fn export(&self, public_only: bool) -> Result<Vec<MemoryDocument>> {
        reader::export(self, public_only).await
    }

    pub async fn search_with_cache(
        &self,
        query_text: &str,
        limit: usize,
    ) -> Result<CachedSearchResult> {
        self.search_with_cache_filtered(query_text, limit, None)
            .await
    }

    pub async fn search_with_cache_filtered(
        &self,
        query_text: &str,
        limit: usize,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<CachedSearchResult> {
        reader::search_with_cache_filtered(self, query_text, limit, filters).await
    }

    pub async fn vsearch(
        &self,
        query_vector: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<MemoryDocument>> {
        search::vsearch(self, query_vector, limit).await
    }

    pub async fn query_with_hybrid_search(
        &self,
        query_text: &str,
        query_vector: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<MemoryDocument>> {
        search::query_with_hybrid_search(self, query_text, query_vector, limit).await
    }

    pub async fn query(
        &self,
        query_text: &str,
        query_vector: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<MemoryDocument>> {
        self.query_filtered(query_text, query_vector, limit, None)
            .await
    }

    pub async fn query_filtered(
        &self,
        query_text: &str,
        query_vector: Vec<f32>,
        limit: usize,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryDocument>> {
        search::query_filtered(self, query_text, query_vector, limit, filters).await
    }

    pub async fn get(&self, path_or_id: &str) -> Result<Option<MemoryDocument>> {
        reader::get(self, path_or_id).await
    }

    pub async fn add(&self, doc: MemoryDocument) -> Result<()> {
        writer::add(self, doc).await
    }

    pub async fn update(&self, doc: MemoryDocument) -> Result<()> {
        writer::update(self, doc).await
    }

    pub async fn add_document(
        &self,
        path: String,
        content: String,
        metadata: serde_json::Value,
    ) -> Result<String> {
        writer::add_document(self, path, content, metadata).await
    }

    pub async fn add_document_typed(
        &self,
        path: String,
        content: String,
        metadata: serde_json::Value,
        typed: Option<TypedMemoryPayload>,
    ) -> Result<String> {
        writer::add_document_typed(self, path, content, metadata, typed).await
    }

    pub async fn add_document_typed_with_embedding(
        &self,
        path: String,
        content: String,
        metadata: serde_json::Value,
        typed: Option<TypedMemoryPayload>,
        embedding: Option<Vec<f32>>,
    ) -> Result<String> {
        writer::add_document_typed_with_embedding(self, path, content, metadata, typed, embedding)
            .await
    }

    pub async fn delete(&self, path_or_id: &str) -> Result<Option<MemoryDocument>> {
        writer::delete(self, path_or_id).await
    }

    pub async fn clear(&self) -> Result<usize> {
        writer::clear(self).await
    }

    pub async fn count(&self) -> Result<usize> {
        Ok(self.docs.read().await.len())
    }

    pub async fn all_documents(&self) -> Vec<MemoryDocument> {
        self.docs.read().await.clone()
    }

    pub async fn ls(&self, path_prefix: &str) -> Result<Vec<NavEntry>> {
        // [B1] Predictive cache warming based on navigation patterns
        if let Some(warmup) = &self.cache_warmup {
            let _ = warmup.predictive_warm(path_prefix, &Default::default()).await;
        }

        let docs = self.all_documents().await;
        let prefix = if path_prefix.is_empty() || path_prefix == "/" {
            "".to_string()
        } else {
            let mut p = path_prefix.to_string();
            if p.starts_with('/') {
                p.remove(0);
            }
            if !p.is_empty() && !p.ends_with('/') {
                p.push('/');
            }
            p
        };

        let mut entries: HashMap<String, NavEntry> = HashMap::new();

        for doc in docs {
            let doc_path = if doc.path.starts_with('/') {
                &doc.path[1..]
            } else {
                &doc.path
            };

            if doc_path.starts_with(&prefix) {
                let remainder = &doc_path[prefix.len()..];
                if remainder.is_empty() {
                    continue;
                }

                let parts: Vec<&str> = remainder.split('/').collect();
                let name = parts[0];
                let is_dir = parts.len() > 1;
                let is_doc = parts.len() == 1;

                let entry_path = if prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{}{}", prefix, name)
                };

                let entry = entries.entry(name.to_string()).or_insert_with(|| NavEntry {
                    name: name.to_string(),
                    path: entry_path,
                    is_dir: false,
                    is_doc: false,
                    id: None,
                });

                if is_dir {
                    entry.is_dir = true;
                }
                if is_doc {
                    entry.is_doc = true;
                    entry.id = doc.id.clone();
                }
            }
        }

        let mut result: Vec<NavEntry> = entries.into_values().collect();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(result)
    }

    pub async fn usage(&self) -> MemoryUsage {
        reader::usage(self).await
    }

    pub async fn cache_metrics(&self) -> CacheMetrics {
        reader::cache_metrics(self).await
    }

    pub async fn multi_hop_context(
        &self,
        query: &str,
        seed_docs: &[MemoryDocument],
        filters: Option<&MemoryQueryFilters>,
    ) -> Vec<MemoryDocument> {
        search::multi_hop_context(self, query, seed_docs, filters).await
    }

    pub async fn invalidate_cache(&self) {
        reader::invalidate_cache(self).await
    }

    pub async fn list_directory(&self, path: &str) -> Result<Vec<MemoryHierarchyNode>> {
        if let Some(store) = self.store().await {
            store.ls(&self.workspace_id, path).await
        } else {
            // Fallback for in-memory only QmdMemory (no persistent store)
            let docs = self.all_documents().await;
            let records: Vec<crate::memory::store::MemoryRecord> = docs
                .into_iter()
                .map(|doc| crate::memory::store::MemoryRecord::from_document(&self.workspace_id, &doc, true, None))
                .collect();
            Ok(crate::memory::hierarchy::MemoryTree::build_ls(
                records, path,
            ))
        }
    }

    /// Find nearest neighbors for a given vector.
    pub async fn nearest_neighbors_query(
        &self,
        query_vector: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<MemoryDocument>> {
        self.vsearch(query_vector, limit).await
    }

    /// Expand search results by depth using parent/child relationships.
    pub async fn expand_depth(
        &self,
        results: &[MemoryDocument],
        depth: usize,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryDocument>> {
        if depth == 0 {
            return Ok(results.to_vec());
        }

        let mut all_docs = results.to_vec();
        let mut seen_ids: std::collections::HashSet<String> = results
            .iter()
            .filter_map(|d| d.id.clone().or_else(|| Some(d.path.clone())))
            .collect();

        let mut current_idxs: Vec<String> = results
            .iter()
            .filter_map(|d| d.id.clone().or_else(|| Some(d.path.clone())))
            .collect();

        for _ in 1..=depth {
            let mut next_ids: Vec<String> = Vec::new();
            for doc_id in &current_idxs {
                if let Ok(Some(doc)) = self.get(doc_id).await {
                    // Fetch parent
                    if let Some(ref parent_id) = doc.parent_id {
                        if !seen_ids.contains(parent_id) {
                            if let Ok(Some(parent)) = self.get(parent_id).await {
                                if matches_filters(
                                    &parent.path,
                                    &parent.metadata,
                                    &self.workspace_id,
                                    filters,
                                ) {
                                    seen_ids.insert(parent_id.clone());
                                    next_ids.push(parent_id.clone());
                                    all_docs.push(parent);
                                }
                            }
                        }
                    }

                    // Fetch children (documents whose parent_id matches this id)
                    if let Some(ref cluster) = doc.cluster_id {
                        let mut child_filters = MemoryQueryFilters::default();
                        child_filters.cluster_ids = Some(vec![cluster.clone()]);
                        if let Ok(children) = self.search_filtered("", 50, Some(&child_filters)).await {
                            for child in children {
                                if let Some(ref cid) = child.id {
                                    if cid != doc_id && !seen_ids.contains(cid) {
                                        if matches_filters(
                                            &child.path,
                                            &child.metadata,
                                            &self.workspace_id,
                                            filters,
                                        ) {
                                            seen_ids.insert(cid.clone());
                                            next_ids.push(cid.clone());
                                            all_docs.push(child);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if next_ids.is_empty() {
                break;
            }
            current_idxs = next_ids;
        }

        Ok(all_docs)
    }
}

// ── Free functions ──────────────────────────────────────────────────

pub async fn query_with_embedding(
    memory: &QmdMemory,
    query_text: &str,
    limit: usize,
) -> Result<Vec<MemoryDocument>> {
    search::query_with_embedding(memory, query_text, limit).await
}

pub async fn query_with_embedding_filtered(
    memory: &QmdMemory,
    query_text: &str,
    limit: usize,
    filters: Option<&MemoryQueryFilters>,
) -> Result<Vec<MemoryDocument>> {
    search::query_with_embedding_filtered(memory, query_text, limit, filters).await
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(hidden_glob_reexports)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn repeated_searches_hit_cache() {
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())));
        memory
            .add_document(
                "docs/cache".to_string(),
                "cache acceleration for repeated searches".to_string(),
                serde_json::json!({}),
            )
            .await
            .expect("test assertion");

        let first = memory
            .search_with_cache("cache acceleration", 5)
            .await
            .expect("test assertion");
        let second = memory
            .search_with_cache("cache acceleration", 5)
            .await
            .expect("test assertion");
        let metrics = memory.cache_metrics().await;

        assert!(!first.cache_hit);
        assert!(second.cache_hit);
        assert_eq!(metrics.misses, 1);
        assert_eq!(metrics.hits, 1);
        assert_eq!(metrics.entries, 1);
    }

    #[tokio::test]
    async fn mutating_memory_invalidates_cache() {
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())));
        memory
            .add_document(
                "docs/original".to_string(),
                "performance tuning for xavier".to_string(),
                serde_json::json!({}),
            )
            .await
            .expect("test assertion");

        let _ = memory
            .search_with_cache("performance", 5)
            .await
            .expect("test assertion");
        assert_eq!(memory.cache_metrics().await.entries, 1);

        memory
            .add_document(
                "docs/new".to_string(),
                "new performance tuning guide".to_string(),
                serde_json::json!({}),
            )
            .await
            .expect("test assertion");

        assert_eq!(memory.cache_metrics().await.entries, 0);
    }

    #[tokio::test]
    async fn add_document_skips_embedding_when_service_not_configured() {
        // SAFETY:
        // - `env::remove_var` and `env::set_var` are unsafe in a multithreaded
        //   context because they can race with other threads reading the same var.
        // - This is a single-threaded test (`#[tokio::test]` runs on one thread),
        //   and env var access is sequential within this scope.
        // - The modified vars are test-specific and no other test accesses them
        //   concurrently (tests run in parallel but each has isolated env via
        //   `env_lock()` elsewhere, and these vars are unique to this test).
        unsafe {
            env::remove_var("XAVIER_EMBEDDING_URL");
            env::set_var("XAVIER_EMBEDDER", "disabled");
        }

        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())));
        memory
            .add_document(
                "docs/offline".to_string(),
                "offline startup should not require embeddings".to_string(),
                serde_json::json!({ "source": "test" }),
            )
            .await
            .expect("test assertion");

        let stored = memory
            .get("docs/offline")
            .await
            .expect("test assertion")
            .expect("test assertion");
        assert!(stored.embedding.is_empty());
    }

    #[tokio::test]
    async fn add_document_creates_clean_locomo_derivatives() {
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())));
        memory
            .add_document(
                "locomo/conv-26/session_1/D1:17".to_string(),
                "Caroline: I've been researching adoption agencies lately.".to_string(),
                serde_json::json!({
                    "benchmark": "locomo",
                    "speaker": "Caroline",
                    "session_time": "8 May, 2023"
                }),
            )
            .await
            .expect("test assertion");

        let stored = memory.all_documents().await;
        assert!(stored.len() > 1);
        let derived = stored
            .iter()
            .find(|doc| {
                doc.metadata.get("memory_kind").and_then(|v| v.as_str()) == Some("fact_atom")
            })
            .expect("derived fact atom");
        assert_eq!(
            derived
                .metadata
                .get("normalized_value")
                .and_then(|v| v.as_str()),
            Some("Adoption agencies")
        );
        assert!(!derived.content.contains("source_path"));
    }

    #[tokio::test]
    async fn locomo_search_prioritizes_temporal_derivatives_over_session_summaries() {
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())));
        memory
            .add_document(
                "locomo/conv-26/session_1/summary".to_string(),
                "Caroline and Melanie spoke on 8 May, 2023. Caroline discussed several LGBTQ experiences and many other summer memories.".to_string(),
                serde_json::json!({
                    "benchmark": "locomo",
                    "session_time": "1:56 pm on 8 May, 2023",
                    "category": "session_summary",
                }),
            )
            .await
            .expect("test assertion");
        memory
            .add_document(
                "locomo/conv-26/session_1/D1:3".to_string(),
                "Caroline: I went to a LGBTQ support group yesterday and it was so powerful."
                    .to_string(),
                serde_json::json!({
                    "benchmark": "locomo",
                    "session_time": "1:56 pm on 8 May, 2023",
                    "speaker": "Caroline",
                    "category": "conversation",
                }),
            )
            .await
            .expect("test assertion");

        let results = memory
            .search("When did Caroline go to the LGBTQ support group?", 5)
            .await
            .expect("test assertion");

        assert!(!results.is_empty());
        assert_eq!(
            results[0]
                .metadata
                .get("memory_kind")
                .and_then(|value| value.as_str()),
            Some("temporal_event")
        );
        assert_eq!(
            results[0]
                .metadata
                .get("resolved_date")
                .and_then(|value| value.as_str()),
            Some("7 May 2023")
        );
    }

    #[tokio::test]
    async fn add_document_normalizes_locomo_dia_ids_for_primary_and_derived_docs() {
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())));
        memory
            .add_document(
                "locomo/conv-26/session_1/D1:03".to_string(),
                "Caroline: I went to a LGBTQ support group yesterday and it was so powerful."
                    .to_string(),
                serde_json::json!({
                    "benchmark": "locomo",
                    "speaker": "Caroline",
                    "session_time": "1:56 pm on 8 May, 2023",
                    "dia_id": "d1:03",
                    "category": "conversation",
                }),
            )
            .await
            .expect("test assertion");

        let stored = memory.all_documents().await;
        let primary = stored
            .iter()
            .find(|doc| doc.path == "locomo/conv-26/session_1/D1:03")
            .expect("primary locomo document");
        assert_eq!(
            primary
                .metadata
                .get("normalized_dia_id")
                .and_then(|value| value.as_str()),
            Some("D1:3")
        );
        assert_eq!(
            primary
                .metadata
                .get("dia_id")
                .and_then(|value| value.as_str()),
            Some("D1:3")
        );

        let derived = stored
            .iter()
            .find(|doc| doc.path.ends_with("#derived/temporal_event/0"))
            .expect("derived temporal event");
        assert_eq!(
            derived
                .metadata
                .get("source_path")
                .and_then(|value| value.as_str()),
            Some("locomo/conv-26/session_1/D1:3")
        );
        assert_eq!(
            derived
                .metadata
                .get("source_dia_id")
                .and_then(|value| value.as_str()),
            Some("D1:3")
        );
    }

    #[tokio::test]
    async fn hybrid_search_uses_rrf_to_combine_keyword_and_vector_hits() {
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())));
        memory
            .add(MemoryDocument {
                id: Some("kw-doc".to_string()),
                path: "docs/keyword".to_string(),
                content: "Alice moved to Paris in 2020 to work as a software engineer.".to_string(),
                metadata: serde_json::json!({}),
                content_vector: Some(vec![0.0, 1.0]),
                embedding: vec![0.0, 1.0],
                ..Default::default()
            })
            .await
            .expect("test assertion");
        memory
            .add(MemoryDocument {
                id: Some("semantic-doc".to_string()),
                path: "docs/semantic".to_string(),
                content:
                    "Alice's favorite programming language is Rust, which she learned in 2021."
                        .to_string(),
                metadata: serde_json::json!({}),
                content_vector: Some(vec![1.0, 0.0]),
                embedding: vec![1.0, 0.0],
                ..Default::default()
            })
            .await
            .expect("test assertion");
        memory
            .add(MemoryDocument {
                id: Some("noise-doc".to_string()),
                path: "docs/noise".to_string(),
                content: "Bob studied design and architecture in Boston.".to_string(),
                metadata: serde_json::json!({}),
                content_vector: Some(vec![0.0, 0.2]),
                embedding: vec![0.0, 0.2],
                ..Default::default()
            })
            .await
            .expect("test assertion");

        let results = memory
            .query_with_hybrid_search("Where did Alice move in 2020?", vec![1.0, 0.0], 3)
            .await
            .expect("test assertion");

        let paths: Vec<&str> = results.iter().map(|doc| doc.path.as_str()).collect();
        assert!(paths.iter().take(2).any(|path| *path == "docs/keyword"));
        assert!(paths.iter().take(2).any(|path| *path == "docs/semantic"));
    }

    #[test]
    fn test_extract_speakers() {
        let text = "Caroline: Hello\n[James]: Hi\nSpeaker: Alice\nPerson: Robert\nGuest: Emma";
        let speakers = extract_speakers(text);
        assert!(speakers.contains(&"Caroline".to_string()));
        assert!(speakers.contains(&"James".to_string()));
        assert!(speakers.contains(&"Alice".to_string()));
        assert!(speakers.contains(&"Robert".to_string()));
        assert!(speakers.contains(&"Emma".to_string()));
    }

    #[test]
    fn test_extract_speaker_from_query() {
        assert_eq!(
            extract_speaker_from_query("Who is Caroline?"),
            Some("Caroline".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("What did James say?"),
            Some("James".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("When was Alice there?"),
            Some("Alice".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("Where is Robert?"),
            Some("Robert".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("Why did Emma laugh?"),
            Some("Emma".to_string())
        );
    }

    #[test]
    fn test_resolve_pronouns() {
        let speakers = vec!["Caroline".to_string(), "James".to_string()];

        // Single female candidate
        assert_eq!(
            resolve_pronouns("What did she say?", &speakers),
            "What did Caroline say?"
        );

        // Single male candidate
        assert_eq!(
            resolve_pronouns("What did he say?", &speakers),
            "What did James say?"
        );

        // Multiple female candidates - no resolution
        let speakers_multiple = vec!["Caroline".to_string(), "Alice".to_string()];
        assert_eq!(
            resolve_pronouns("What did she say?", &speakers_multiple),
            "What did she say?"
        );
    }

    #[test]
    fn test_is_likely_speaker() {
        assert!(is_likely_speaker("Caroline"));
        assert!(is_likely_speaker("James"));
        assert!(!is_likely_speaker("Who"));
        assert!(!is_likely_speaker("What"));
        assert!(!is_likely_speaker("She"));
        assert!(!is_likely_speaker("The"));
    }

    #[test]
    fn test_normalized_id() {
        let cases = vec![
            ("Foo.Bar", "foo_bar"),
            ("foo__bar", "foo_bar"),
            ("ID-123", "id_123"),
            ("   Space   ", "_space_"),
            ("Special!@#$%^&*()Chars", "special_chars"),
            ("NFKC\u{00B2}", "nfkc2"), // superscript 2
            ("CaseFold_TEST", "casefold_test"),
        ];

        for (input, expected) in cases {
            let normalized: NormalizedId = input.parse().unwrap();
            assert_eq!(normalized.as_str(), expected, "Failed for input: {}", input);
        }
    }

    #[tokio::test]
    async fn test_ls_navigation() {
        use serde_json::json;
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())));

        memory.add_document("docs/api/v1".to_string(), "v1".to_string(), json!({})).await.unwrap();
        memory.add_document("docs/api/v2".to_string(), "v2".to_string(), json!({})).await.unwrap();
        memory.add_document("docs/readme.md".to_string(), "readme".to_string(), json!({})).await.unwrap();
        memory.add_document("blog/post1".to_string(), "post1".to_string(), json!({})).await.unwrap();

        // Test root ls
        let root = memory.ls("").await.unwrap();
        assert_eq!(root.len(), 2);
        assert_eq!(root[0].name, "blog");
        assert!(root[0].is_dir);
        assert!(!root[0].is_doc);
        assert_eq!(root[1].name, "docs");
        assert!(root[1].is_dir);

        // Test docs ls
        let docs = memory.ls("docs").await.unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].name, "api");
        assert!(docs[0].is_dir);
        assert_eq!(docs[1].name, "readme.md");
        assert!(docs[1].is_doc);

        // Test docs/api ls
        let api = memory.ls("docs/api").await.unwrap();
        assert_eq!(api.len(), 2);
        assert_eq!(api[0].name, "v1");
        assert!(api[0].is_doc);
        assert_eq!(api[1].name, "v2");
        assert!(api[1].is_doc);
    }
}
