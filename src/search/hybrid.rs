//! Hybrid search implementation
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::{
    embedding,
    memory::{
        qmd_memory::{MemoryDocument, QmdMemory},
        schema::MemoryQueryFilters,
    },
};

use super::hooks::HookRegistry;
use super::rrf::{reciprocal_rank_fusion_weighted, ScoredResult};

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("embedding error: {0}")]
    Embedding(String),
    #[error("search error: {0}")]
    Search(String),
    #[error("hook error: {0}")]
    Hook(String),
}

use moka::future::Cache;
use std::sync::LazyLock;
use std::time::Duration;

static HYBRID_SEARCH_CACHE: LazyLock<Cache<String, Vec<ScoredResult>>> = LazyLock::new(|| {
    Cache::builder()
        .max_capacity(1000)
        .time_to_live(Duration::from_secs(3600))
        .build()
});

/// Invalidate hybrid cache.
pub async fn invalidate_hybrid_cache() {
    HYBRID_SEARCH_CACHE.invalidate_all();
}

#[derive(Clone)]
pub struct HybridSearcher {
    pub keyword_weight: f32,
    pub vector_weight: f32,
    pub rrf_k: u32,
    pub hooks: HookRegistry,
    pub embedder: Option<std::sync::Arc<dyn crate::embedding::Embedder>>,
}

impl std::fmt::Debug for HybridSearcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HybridSearcher")
            .field("keyword_weight", &self.keyword_weight)
            .field("vector_weight", &self.vector_weight)
            .field("rrf_k", &self.rrf_k)
            .field("hooks", &self.hooks)
            .field("embedder_configured", &self.embedder.is_some())
            .finish()
    }
}

impl Default for HybridSearcher {
    fn default() -> Self {
        let mut hooks = HookRegistry::new();
        if let Some(rerank_hook) = crate::search::rerank::RerankHook::from_env() {
            hooks.add_hook(std::sync::Arc::new(rerank_hook));
        }

        Self {
            keyword_weight: crate::retrieval::config::configured_keyword_weight(),
            vector_weight: crate::retrieval::config::configured_vector_weight(),
            rrf_k: configured_rrf_k(),
            hooks,
            embedder: None,
        }
    }
}

/// Configured rrf k.
pub fn configured_rrf_k() -> u32 {
    crate::retrieval::config::configured_rrf_k()
}

impl HybridSearcher {
    /// New.
    pub fn new() -> Self {
        Self::default()
    }

    /// Search.
    pub async fn search(
        &self,
        memory: &QmdMemory,
        query: &str,
        limit: usize,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<ScoredResult>, SearchError> {
        let mut query = query.to_string();
        let mut filters = filters.cloned();

        // Execute pre-query hooks
        self.hooks
            .execute_pre_query(&mut query, &mut filters)
            .await
            .map_err(|e| SearchError::Hook(e.to_string()))?;

        // Check cache
        let cache_key = format!(
            "{}:{}:{}:{}:{}:{:?}",
            memory.workspace_id(),
            query,
            limit,
            self.keyword_weight,
            self.vector_weight,
            filters
        );
        if let Some(results) = HYBRID_SEARCH_CACHE.get(&cache_key).await {
            return Ok(results);
        }

        // Use a larger candidate pool if reranking is likely to be involved
        let candidate_limit = if !self.hooks.is_empty() {
            limit * 3
        } else {
            limit * 2
        };

        let keyword_results = self
            .keyword_search(memory, &query, candidate_limit, filters.as_ref())
            .await?;
        let vector_results = self
            .vector_search(memory, &query, candidate_limit, filters.as_ref())
            .await
            .unwrap_or_default();

        if keyword_results.is_empty() && vector_results.is_empty() {
            return Ok(Vec::new());
        }

        let mut fused = reciprocal_rank_fusion_weighted(
            vec![
                (keyword_results, self.keyword_weight),
                (vector_results, self.vector_weight),
            ],
            self.rrf_k,
        );

        // Execute post-query hooks
        self.hooks
            .execute_post_query(&query, &mut fused)
            .await
            .map_err(|e| SearchError::Hook(e.to_string()))?;

        let final_results: Vec<ScoredResult> = fused.into_iter().take(limit).collect();

        // Update cache
        HYBRID_SEARCH_CACHE
            .insert(cache_key, final_results.clone())
            .await;

        Ok(final_results)
    }

    async fn keyword_search(
        &self,
        memory: &QmdMemory,
        query: &str,
        limit: usize,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<ScoredResult>, SearchError> {
        let documents = memory
            .bm25_search(query, limit, filters)
            .await
            .map_err(|error| SearchError::Search(error.to_string()))?;

        Ok(self
            .convert_documents(documents, "keyword")
            .into_iter()
            .collect())
    }

    async fn vector_search(
        &self,
        memory: &QmdMemory,
        query: &str,
        limit: usize,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<ScoredResult>, SearchError> {
        let embedder = if let Some(embedder) = &self.embedder {
            embedder.clone()
        } else {
            embedding::build_embedder_from_env()
                .await
                .map_err(|error| SearchError::Embedding(error.to_string()))?
        };

        let query_vector = match embedder.encode(query).await {
            Ok(vector) => vector,
            Err(error) => {
                tracing::warn!(error = %error, "vector search: embedding failed; skipping semantic results");
                return Ok(Vec::new());
            }
        };

        if query_vector.is_empty() {
            return Ok(Vec::new());
        }

        let documents = memory
            .vsearch(query_vector, limit)
            .await
            .map_err(|error| SearchError::Search(error.to_string()))?
            .into_iter()
            .filter(|doc| {
                filters
                    .map(|filters| {
                        crate::memory::schema::matches_filters(
                            &doc.path,
                            &doc.metadata,
                            memory.workspace_id(),
                            Some(filters),
                        )
                    })
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();

        Ok(self.convert_documents(documents, "vector"))
    }

    fn convert_documents(&self, documents: Vec<MemoryDocument>, source: &str) -> Vec<ScoredResult> {
        let total = documents.len().max(1) as f32;
        documents
            .into_iter()
            .enumerate()
            .map(|(index, document)| {
                let score = 1.0 - (index as f32 / total);
                let updated_at = document
                    .metadata
                    .get("updated_at")
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.timestamp_millis());
                ScoredResult {
                    id: document.id.unwrap_or_else(|| document.path.clone()),
                    content: document.content,
                    score,
                    source: source.to_string(),
                    path: document.path,
                    updated_at,
                    zone: document
                        .metadata
                        .get("zone")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::qmd_memory::QmdMemory;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_hybrid_search_basic() {
        let memory = QmdMemory::new(Arc::new(RwLock::new(Vec::new())));
        memory
            .add_document(
                "doc1".to_string(),
                "the quick brown fox".to_string(),
                serde_json::json!({}),
            )
            .await
            .expect("test assertion");
        memory
            .add_document(
                "doc2".to_string(),
                "the lazy dog".to_string(),
                serde_json::json!({}),
            )
            .await
            .expect("test assertion");

        let searcher = HybridSearcher::new();
        let results = searcher
            .search(&memory, "quick", 10, None)
            .await
            .expect("test assertion");

        assert!(!results.is_empty());
        assert_eq!(results[0].path, "doc1");
    }

    struct QueryExpander;
    #[async_trait::async_trait]
    impl crate::search::hooks::SearchHook for QueryExpander {
        fn name(&self) -> &str {
            "expander"
        }
        async fn pre_query(
            &self,
            query: &mut String,
            _filters: &mut Option<MemoryQueryFilters>,
        ) -> anyhow::Result<()> {
            if query == "fast" {
                *query = "quick".to_string();
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_hybrid_search_with_hooks() {
        let memory = QmdMemory::new(Arc::new(RwLock::new(Vec::new())));
        memory
            .add_document(
                "doc1".to_string(),
                "the quick brown fox".to_string(),
                serde_json::json!({}),
            )
            .await
            .expect("test assertion");

        let mut searcher = HybridSearcher::new();
        searcher.hooks.add_hook(Arc::new(QueryExpander));

        // "fast" should be expanded to "quick" and match doc1
        let results = searcher
            .search(&memory, "fast", 10, None)
            .await
            .expect("test assertion");
        assert!(!results.is_empty());
        assert_eq!(results[0].path, "doc1");
    }

    #[test]
    fn test_configured_rrf_k_from_env() {
        std::env::set_var("XAVIER_RRF_K", "100");
        assert_eq!(configured_rrf_k(), 100);
        std::env::remove_var("XAVIER_RRF_K");
        assert_eq!(configured_rrf_k(), 60);
    }

    #[tokio::test]
    async fn test_hybrid_search_cache() {
        let memory = QmdMemory::new(Arc::new(RwLock::new(Vec::new())));
        memory
            .add_document(
                "doc1".to_string(),
                "the quick brown fox".to_string(),
                serde_json::json!({}),
            )
            .await
            .expect("test assertion");

        let searcher = HybridSearcher::new();

        // First search - should populate cache
        let results1 = searcher
            .search(&memory, "quick", 10, None)
            .await
            .expect("test assertion");

        // Second search - should hit cache
        let results2 = searcher
            .search(&memory, "quick", 10, None)
            .await
            .expect("test assertion");

        assert_eq!(results1.len(), results2.len());
        // Since we are using QmdMemory::add_document, it generates a new ULID ID every time if not specified.
        // But the search results content and path should be identical.
        assert_eq!(results1[0].content, results2[0].content);
        assert_eq!(results1[0].path, results2[0].path);
    }
}
