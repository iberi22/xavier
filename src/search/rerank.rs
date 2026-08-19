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
}
