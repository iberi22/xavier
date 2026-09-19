//! DocBot RAG pipeline.
//!
//! End-to-end pipeline: query → BM25 search → (optional vector search) →
//! rerank → prompt assembly → LLM completion → cited answer.

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{debug, info};

use crate::collections::schema::{AskRequest, AskResponse, CitedSource};
use crate::collections::store::CollectionStore;
use crate::rag::llm_adapter::{LlmAdapter, LlmAdapterTrait, LlmConfig};
use crate::rag::prompt::{PromptBuilder, PromptLanguage, RetrievedChunk};

// ── Pipeline Config ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Number of chunks to retrieve before reranking.
    pub top_k_retrieve: usize,
    /// Number of chunks to pass to the LLM after reranking.
    pub top_k_context: usize,
    /// Score threshold — discard chunks below this score.
    pub min_score: f32,
    /// Maximum excerpt chars in cited sources.
    pub excerpt_len: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            top_k_retrieve: 20,
            top_k_context: 5,
            min_score: -10.0, // BM25 can go negative; keep permissive default
            excerpt_len: 200,
        }
    }
}

// ── Pipeline ───────────────────────────────────────────────────────────────────

/// The main DocBot RAG pipeline.
///
/// Holds shared references to the collection store and LLM adapter so it can
/// be cloned across Axum handlers and gateway tasks.
#[derive(Clone)]
pub struct DocBotPipeline {
    store: Arc<CollectionStore>,
    llm: Arc<LlmAdapter>,
    config: PipelineConfig,
}

impl DocBotPipeline {
    pub fn new(
        store: Arc<CollectionStore>,
        llm_config: LlmConfig,
        pipeline_config: PipelineConfig,
    ) -> Self {
        let llm = Arc::new(LlmAdapter::from_config(llm_config));
        Self {
            store,
            llm,
            config: pipeline_config,
        }
    }

    pub fn from_env(store: Arc<CollectionStore>) -> Self {
        Self::new(store, LlmConfig::from_env(), PipelineConfig::default())
    }

    /// Run the RAG pipeline for an `AskRequest`.
    pub async fn ask(&self, req: &AskRequest) -> Result<AskResponse> {
        let top_k = req.top_k.unwrap_or(self.config.top_k_retrieve);

        // 1. BM25 search
        let bm25_hits = self
            .store
            .fts_search(&req.query, req.collection_id.as_deref(), top_k)
            .context("BM25 search failed")?;

        debug!(
            query = %req.query,
            hits = bm25_hits.len(),
            "BM25 search complete"
        );

        if bm25_hits.is_empty() {
            let lang = req
                .language
                .as_deref()
                .map(PromptLanguage::from_str)
                .unwrap_or_default();
            let answer = match lang {
                PromptLanguage::Spanish => {
                    "No encontré información sobre eso en los documentos disponibles.".to_string()
                }
                PromptLanguage::English => {
                    "I could not find information about that in the available documents."
                        .to_string()
                }
            };
            return Ok(AskResponse {
                answer,
                sources: vec![],
                query: req.query.clone(),
                model_used: Some(self.llm.model_name().to_string()),
            });
        }

        // 2. Map BM25 hits to RetrievedChunk (enrich with doc title from DB)
        let mut retrieved: Vec<RetrievedChunk> = bm25_hits
            .into_iter()
            .filter(|h| h.score >= self.config.min_score)
            .take(self.config.top_k_context)
            .map(|h| {
                // Fetch doc title (fallback to doc_id if not found)
                let doc_title = self
                    .store
                    .get_doc(&h.doc_id)
                    .ok()
                    .flatten()
                    .map(|d| d.title)
                    .unwrap_or_else(|| h.doc_id.clone());

                let col_name = self
                    .store
                    .get_collection(&h.collection_id)
                    .ok()
                    .flatten()
                    .map(|c| c.name)
                    .unwrap_or_else(|| h.collection_id.clone());

                RetrievedChunk {
                    chunk_id: h.chunk_id,
                    doc_id: h.doc_id,
                    doc_title,
                    collection_id: h.collection_id,
                    collection_name: col_name,
                    content: h.content,
                    page: h.page,
                    breadcrumb: h.breadcrumb,
                    score: h.score,
                }
            })
            .collect();

        // 3. Sort by score descending (BM25 lower = better, so sort ascending)
        retrieved.sort_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 4. Assemble prompt
        let lang = req
            .language
            .as_deref()
            .map(PromptLanguage::from_str)
            .unwrap_or_default();
        let builder = PromptBuilder::new(lang);
        let prompt = builder.build(&req.query, &retrieved);

        debug!(prompt_len = prompt.len(), "prompt assembled");

        // 5. LLM completion
        let answer = self.llm.complete(&prompt).await.unwrap_or_else(|e| {
            tracing::warn!(error = %e, "LLM failed, falling back to context");
            retrieved
                .first()
                .map(|c| c.content.clone())
                .unwrap_or_default()
        });

        // 6. Build cited sources
        let sources: Vec<CitedSource> = retrieved
            .iter()
            .map(|c| c.to_cited_source(self.config.excerpt_len))
            .collect();

        info!(
            query = %req.query,
            chunks_used = sources.len(),
            model = %self.llm.model_name(),
            "RAG pipeline complete"
        );

        Ok(AskResponse {
            answer,
            sources,
            query: req.query.clone(),
            model_used: Some(self.llm.model_name().to_string()),
        })
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::schema::{DocCollection, DocEntry};
    use crate::collections::store::CollectionStore;
    use crate::collections::DocChunk;
    use crate::rag::llm_adapter::LlmBackend;
    use tempfile::tempdir;

    fn make_store() -> Arc<CollectionStore> {
        let dir = tempdir().unwrap();
        let path = dir.path().join("pipeline_test.db");
        Arc::new(CollectionStore::open(&path).unwrap())
    }

    fn retrieval_only_config() -> LlmConfig {
        LlmConfig {
            backend: LlmBackend::RetrievalOnly,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_ask_no_documents_returns_empty_answer() {
        let store = make_store();
        let pipeline = DocBotPipeline::new(store, retrieval_only_config(), Default::default());

        let req = AskRequest {
            query: "¿Qué dice el artículo 5?".to_string(),
            collection_id: None,
            top_k: Some(5),
            language: Some("es".to_string()),
        };

        let resp = pipeline.ask(&req).await.unwrap();
        assert!(resp.answer.contains("No encontré"));
        assert!(resp.sources.is_empty());
    }

    #[tokio::test]
    async fn test_ask_with_indexed_document() {
        let store = make_store();

        // Set up collection + doc + chunk
        let col = DocCollection::new("Test Col");
        store.create_collection(&col).unwrap();

        let doc = DocEntry::new(&col.id, "Contrato Marco", "application/pdf");
        store.create_doc(&doc).unwrap();

        let chunk = DocChunk::new(
            &doc.id,
            &col.id,
            0,
            "El arrendatario pagará mil pesos mensuales según el artículo cinco.",
        )
        .with_page(2)
        .with_breadcrumb("Artículo 5");
        store.insert_chunks(&[chunk]).unwrap();

        let pipeline = DocBotPipeline::new(
            store,
            retrieval_only_config(),
            PipelineConfig {
                top_k_retrieve: 5,
                top_k_context: 3,
                ..Default::default()
            },
        );

        let req = AskRequest {
            query: "artículo cinco arrendatario".to_string(),
            collection_id: Some(col.id.clone()),
            top_k: Some(5),
            language: Some("es".to_string()),
        };

        let resp = pipeline.ask(&req).await.unwrap();
        // With retrieval-only, answer should contain context text
        assert!(!resp.answer.is_empty());
        assert_eq!(resp.query, "artículo cinco arrendatario");
    }
}
