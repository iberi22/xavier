//! Search result reranking
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::rrf::ScoredResult;
use crate::memory::schema::MemoryQueryFilters;
use crate::search::hooks::SearchHook;

#[async_trait]
pub trait Reranker: Send + Sync {
    async fn rerank(&self, query: &str, results: &mut Vec<ScoredResult>) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RerankRequest {
    model: String,
    query: String,
    documents: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_n: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct RerankResponse {
    results: Vec<RerankResult>,
}

#[derive(Debug, Clone, Deserialize)]
struct RerankResult {
    index: usize,
    relevance_score: f32,
}

pub struct HttpReranker {
    endpoint: String,
    model: String,
    api_key: Option<String>,
}

impl HttpReranker {
    /// New.
    pub fn new(endpoint: String, model: String, api_key: Option<String>) -> Self {
        Self {
            endpoint,
            model,
            api_key,
        }
    }

    /// From env.
    pub fn from_env() -> Option<Self> {
        let enabled = std::env::var("XAVIER_RERANK_ENABLED")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        if !enabled {
            return None;
        }

        let endpoint = std::env::var("XAVIER_RERANK_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:11434/v1/rerank".to_string());
        let model = std::env::var("XAVIER_RERANK_MODEL")
            .unwrap_or_else(|_| "mxbai-rerank-base".to_string());
        let api_key = std::env::var("XAVIER_RERANK_API_KEY").ok();

        Some(Self::new(endpoint, model, api_key))
    }
}

#[async_trait]
impl Reranker for HttpReranker {
    async fn rerank(&self, query: &str, results: &mut Vec<ScoredResult>) -> anyhow::Result<()> {
        if results.is_empty() {
            return Ok(());
        }

        let documents: Vec<String> = results.iter().map(|r| r.content.clone()).collect();
        let top_n = results.len();

        let request = RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents,
            top_n: Some(top_n),
        };

        let client = &crate::utils::http::DEFAULT_HTTP_CLIENT;
        let mut req = client.post(&self.endpoint).json(&request);
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }

        let response: RerankResponse = req.send().await?.json().await?;

        // Re-score results based on reranker output
        let mut index_to_score = std::collections::HashMap::new();
        for res in response.results {
            index_to_score.insert(res.index, res.relevance_score);
        }

        for (i, result) in results.iter_mut().enumerate() {
            if let Some(score) = index_to_score.get(&i) {
                result.score = *score;
            }
        }

        // Sort by new score
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(())
    }
}

/// RAG pipeline search hook combining RagHybridConfig + reranker
pub struct RagHook {
    config: RagHybridConfig,
    reranker: Option<Arc<dyn Reranker>>,
}

impl RagHook {
    pub fn new(config: RagHybridConfig, reranker: Option<Arc<dyn Reranker>>) -> Self {
        Self { config, reranker }
    }

    pub fn from_env() -> Self {
        let config = RagHybridConfig::from_env();
        let reranker = HttpReranker::from_env().map(|r| Arc::new(r) as Arc<dyn Reranker>);
        Self::new(config, reranker)
    }
}

#[async_trait]
impl SearchHook for RagHook {
    fn name(&self) -> &str {
        "rag_hybrid"
    }

    async fn pre_query(
        &self,
        query: &mut String,
        _filters: &mut Option<MemoryQueryFilters>,
    ) -> anyhow::Result<()> {
        if self.config.enable_hyde {
            let hypo = hyde_hypothetical_doc(query);
            query.push(' ');
            query.push_str(&hypo);
        }
        Ok(())
    }

    async fn post_query(&self, query: &str, results: &mut Vec<ScoredResult>) -> anyhow::Result<()> {
        let fused = rag_pipeline(query, results.clone(), vec![], vec![], &self.config);
        if !fused.is_empty() {
            *results = fused;
        }
        if self.config.enable_rerank {
            if let Some(reranker) = &self.reranker {
                reranker.rerank(query, results).await?;
            }
        }
        Ok(())
    }
}

pub struct RerankHook {
    reranker: Arc<dyn Reranker>,
}

impl RerankHook {
    /// New.
    pub fn new(reranker: Arc<dyn Reranker>) -> Self {
        Self { reranker }
    }

    /// From env.
    pub fn from_env() -> Option<Self> {
        HttpReranker::from_env().map(|r| Self::new(Arc::new(r)))
    }
}

#[async_trait]
impl SearchHook for RerankHook {
    fn name(&self) -> &str {
        "rerank"
    }

    async fn post_query(&self, query: &str, results: &mut Vec<ScoredResult>) -> anyhow::Result<()> {
        self.reranker.rerank(query, results).await
    }

    async fn pre_query(
        &self,
        _query: &mut String,
        _filters: &mut Option<MemoryQueryFilters>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// RAG hybrid: RRF BM25+vector+code_tokens + local reranker + HyDE (WAVE-3.07)
///
/// Combines three retrieval signals via Reciprocal Rank Fusion (RRF) with
/// code-token boost, then optionally applies a local cross-encoder reranker.
/// HyDE (Hypothetical Document Embeddings) generates a pseudo-document from the
/// query to improve vector recall for vague queries.
#[derive(Debug, Clone)]
pub struct RagHybridConfig {
    pub rrf_k: f32,
    pub code_token_boost: f32,
    pub enable_hyde: bool,
    pub enable_rerank: bool,
    pub hyde_model: Option<String>,
}

impl Default for RagHybridConfig {
    fn default() -> Self {
        Self {
            rrf_k: 60.0,
            code_token_boost: 1.2,
            enable_hyde: false,
            enable_rerank: false,
            hyde_model: None,
        }
    }
}

impl RagHybridConfig {
    pub fn from_env() -> Self {
        let rrf_k = std::env::var("XAVIER_RRF_K")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60.0);
        let hyde = std::env::var("XAVIER_HYDE_ENABLED")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);
        let rerank = std::env::var("XAVIER_RERANK_ENABLED")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);
        Self {
            rrf_k,
            enable_hyde: hyde,
            enable_rerank: rerank,
            ..Default::default()
        }
    }
}

/// RRF fusion: combine BM25 + vector + code-token ranked lists
pub fn rrf_fuse(
    bm25: &[ScoredResult],
    vector: &[ScoredResult],
    code_tokens: &[ScoredResult],
    k: f32,
    code_boost: f32,
) -> Vec<ScoredResult> {
    use std::collections::HashMap;
    let mut scores: HashMap<String, (f32, ScoredResult)> = HashMap::new();
    let lists: Vec<(&[ScoredResult], f32)> =
        vec![(bm25, 1.0), (vector, 1.0), (code_tokens, code_boost)];
    for (list, weight) in lists {
        for (rank, res) in list.iter().enumerate() {
            let rrf_score = weight / (k + rank as f32 + 1.0);
            let entry = scores.entry(res.id.clone()).or_insert((0.0, res.clone()));
            entry.0 += rrf_score;
        }
    }
    let mut fused: Vec<ScoredResult> = scores
        .into_values()
        .map(|(score, mut r)| {
            r.score = score;
            r
        })
        .collect();
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    fused
}

/// HyDE: generate hypothetical document for query expansion (stub local)
pub fn hyde_hypothetical_doc(query: &str) -> String {
    // In production this would call a local LLM to generate a pseudo-doc.
    // Stub: returns query wrapped as hypothetical answer for embedding.
    format!("Hypothetical answer for: {query}. This document would contain relevant context, definitions, and examples related to the query.")
}

/// RAG pipeline helper: optionally expand query via HyDE then fuse
pub fn rag_pipeline(
    query: &str,
    bm25: Vec<ScoredResult>,
    vector: Vec<ScoredResult>,
    code_tokens: Vec<ScoredResult>,
    config: &RagHybridConfig,
) -> Vec<ScoredResult> {
    let _hyde_doc = if config.enable_hyde {
        Some(hyde_hypothetical_doc(query))
    } else {
        None
    };
    // For now HyDE doc is used only as additional vector signal (not implemented).
    // Fuse via RRF.
    rrf_fuse(
        &bm25,
        &vector,
        &code_tokens,
        config.rrf_k,
        config.code_token_boost,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockReranker;
    #[async_trait]
    impl Reranker for MockReranker {
        async fn rerank(
            &self,
            _query: &str,
            results: &mut Vec<ScoredResult>,
        ) -> anyhow::Result<()> {
            // Reverse scores for testing
            let len = results.len();
            for (i, res) in results.iter_mut().enumerate() {
                res.score = (len - i) as f32;
            }
            results.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_rerank_hook() {
        let hook = RerankHook::new(Arc::new(MockReranker));
        let mut results = vec![
            ScoredResult {
                id: "1".into(),
                content: "doc1".into(),
                score: 0.1,
                source: "test".into(),
                path: "path1".into(),
                updated_at: None,
                zone: None,
            },
            ScoredResult {
                id: "2".into(),
                content: "doc2".into(),
                score: 0.9,
                source: "test".into(),
                path: "path2".into(),
                updated_at: None,
                zone: None,
            },
        ];

        hook.post_query("query", &mut results).await.unwrap();

        assert_eq!(results[0].id, "1");
        assert_eq!(results[0].score, 2.0);
        assert_eq!(results[1].id, "2");
        assert_eq!(results[1].score, 1.0);
    }

    #[test]
    fn test_rag_rrf_fuse() {
        let bm25 = vec![
            ScoredResult {
                id: "a".into(),
                content: "a".into(),
                score: 1.0,
                source: "bm25".into(),
                path: "p".into(),
                updated_at: None,
                zone: None,
            },
            ScoredResult {
                id: "b".into(),
                content: "b".into(),
                score: 0.5,
                source: "bm25".into(),
                path: "p".into(),
                updated_at: None,
                zone: None,
            },
        ];
        let vector = vec![
            ScoredResult {
                id: "b".into(),
                content: "b".into(),
                score: 1.0,
                source: "vec".into(),
                path: "p".into(),
                updated_at: None,
                zone: None,
            },
            ScoredResult {
                id: "c".into(),
                content: "c".into(),
                score: 0.8,
                source: "vec".into(),
                path: "p".into(),
                updated_at: None,
                zone: None,
            },
        ];
        let code = vec![ScoredResult {
            id: "a".into(),
            content: "a".into(),
            score: 1.0,
            source: "code".into(),
            path: "p".into(),
            updated_at: None,
            zone: None,
        }];
        let fused = rrf_fuse(&bm25, &vector, &code, 60.0, 1.2);
        assert!(!fused.is_empty());
        // a appears in bm25+code, b in bm25+vector, c only vector — a or b should top
        assert!(fused[0].id == "a" || fused[0].id == "b");
    }

    #[test]
    fn test_rag_hyde() {
        let doc = hyde_hypothetical_doc("quantum computing");
        assert!(doc.contains("quantum computing"));
        assert!(doc.contains("Hypothetical"));
        let cfg = RagHybridConfig {
            enable_hyde: true,
            ..Default::default()
        };
        let fused = rag_pipeline("test", vec![], vec![], vec![], &cfg);
        assert!(fused.is_empty());
        let cfg2 = RagHybridConfig::default();
        assert!(!cfg2.enable_hyde);
    }

    #[tokio::test]
    async fn test_rag_e2e() {
        let bm25 = vec![
            ScoredResult {
                id: "doc_a".into(),
                content: "fn calculate_hash()".into(),
                score: 0.9,
                source: "bm25".into(),
                path: "src/hash.rs".into(),
                updated_at: None,
                zone: None,
            },
            ScoredResult {
                id: "doc_b".into(),
                content: "struct HashConfig".into(),
                score: 0.7,
                source: "bm25".into(),
                path: "src/config.rs".into(),
                updated_at: None,
                zone: None,
            },
        ];

        let vector = vec![
            ScoredResult {
                id: "doc_b".into(),
                content: "struct HashConfig".into(),
                score: 0.85,
                source: "vector".into(),
                path: "src/config.rs".into(),
                updated_at: None,
                zone: None,
            },
            ScoredResult {
                id: "doc_c".into(),
                content: "impl Digest for Hasher".into(),
                score: 0.6,
                source: "vector".into(),
                path: "src/digest.rs".into(),
                updated_at: None,
                zone: None,
            },
        ];

        let code_tokens = vec![ScoredResult {
            id: "doc_a".into(),
            content: "fn calculate_hash()".into(),
            score: 0.95,
            source: "code_tokens".into(),
            path: "src/hash.rs".into(),
            updated_at: None,
            zone: None,
        }];

        let query = "calculate hash digest";
        let config = RagHybridConfig {
            rrf_k: 60.0,
            code_token_boost: 1.5,
            enable_hyde: true,
            enable_rerank: true,
            hyde_model: Some("mxbai-rerank-base".into()),
        };

        // 1. Pipeline fusion
        let fused = rag_pipeline(query, bm25, vector, code_tokens, &config);
        assert_eq!(fused.len(), 3);
        // doc_a appears in BM25 + Code tokens (with 1.5 boost), doc_b in BM25 + Vector
        assert_eq!(fused[0].id, "doc_a");

        // 2. SearchHook E2E invocation with MockReranker
        let hook = RagHook::new(config, Some(Arc::new(MockReranker)));
        let mut hook_results = fused.clone();

        hook.post_query(query, &mut hook_results).await.unwrap();
        assert_eq!(hook_results.len(), 3);
        assert_eq!(hook.name(), "rag_hybrid");

        // Verify recall for all 3 expected items
        let ids: Vec<&str> = hook_results.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"doc_a"));
        assert!(ids.contains(&"doc_b"));
        assert!(ids.contains(&"doc_c"));
    }
}
