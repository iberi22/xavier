//! Memory Query Engine
//!
//! Unified memory search and context extraction engine for HTTP, MCP, and CLI.

use serde::{Deserialize, Serialize};

use crate::memory::{
    qmd_memory::{MemoryDocument, QmdMemory},
    schema::MemoryQueryFilters,
    snippet,
    store::MemoryRecord,
};
use crate::search::hybrid::HybridSearcher;

/// Parameters for memory search requests across API, MCP, and CLI interfaces.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchQuery {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub depth: usize,
    #[serde(default)]
    pub rrf_k: Option<u32>,
    #[serde(default = "default_keyword_weight")]
    pub keyword_weight: f32,
    #[serde(default = "default_vector_weight")]
    pub vector_weight: f32,
    #[serde(default)]
    pub filters: Option<MemoryQueryFilters>,
    #[serde(default)]
    pub include_embedding: Option<bool>,
    #[serde(default)]
    pub include_content: Option<bool>,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            limit: default_limit(),
            depth: 0,
            rrf_k: None,
            keyword_weight: default_keyword_weight(),
            vector_weight: default_vector_weight(),
            filters: None,
            include_embedding: None,
            include_content: None,
        }
    }
}

fn default_keyword_weight() -> f32 {
    0.5
}

fn default_vector_weight() -> f32 {
    0.5
}

fn default_limit() -> usize {
    10
}

/// A single search result item representing candidate documents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub id: String,
    pub path: String,
    pub content: String,
    pub score: f32,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub kind: String,
    pub metadata: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical_score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
}

/// Aggregated search results returned by `MemoryQueryEngine::search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub results: Vec<SearchResultItem>,
    pub query_vector: Option<Vec<f32>>,
    pub total_available: usize,
    pub search_type: String,
}

/// Parameters for context page-in requests.
#[derive(Debug, Clone, Default)]
pub struct ContextParams {
    pub query: Option<String>,
    pub ids: Option<Vec<String>>,
    pub explicit_docs: Option<Vec<MemoryDocument>>,
    pub limit: usize,
    pub max_chars: usize,
    pub max_chars_per_doc: usize,
    pub depth: usize,
    pub filters: Option<MemoryQueryFilters>,
}

/// Aggregated context output produced by `MemoryQueryEngine::context`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContext {
    pub total_chars: usize,
    pub total_records: usize,
    pub truncated: bool,
    pub truncated_reason: Option<String>,
    pub content: String,
    pub sources: Vec<SearchResultItem>,
    pub estimated_tokens: usize,
}

/// Central query engine for unified search and context assembly.
pub struct MemoryQueryEngine;

impl MemoryQueryEngine {
    /// Create a new query engine instance.
    pub fn new() -> Self {
        Self
    }

    /// Execute hybrid search over `QmdMemory`.
    pub async fn search(
        &self,
        memory: &QmdMemory,
        req: SearchQuery,
    ) -> anyhow::Result<SearchResults> {
        let limit = if req.limit == 0 { 10 } else { req.limit };
        let filter_ref = req.filters.as_ref();

        let raw_docs =
            if req.rrf_k.is_some() || req.keyword_weight != 0.5 || req.vector_weight != 0.5 {
                let mut searcher = HybridSearcher::new();
                searcher.keyword_weight = req.keyword_weight;
                searcher.vector_weight = req.vector_weight;
                if let Some(rrf_k) = req.rrf_k {
                    searcher.rrf_k = rrf_k;
                }
                if let Ok(scored) = searcher.search(memory, &req.query, limit, filter_ref).await {
                    let mut docs = Vec::new();
                    for res in scored {
                        let mut doc =
                            memory.get(&res.id).await.ok().flatten().unwrap_or_else(|| {
                                MemoryDocument {
                                    id: Some(res.id.clone()),
                                    path: res.path.clone(),
                                    content: res.content.clone(),
                                    score: res.score,
                                    metadata: serde_json::json!({}),
                                    ..Default::default()
                                }
                            });
                        doc.score = res.score;
                        if let Some(obj) = doc.metadata.as_object_mut() {
                            if !obj.contains_key("source") {
                                obj.insert("source".to_string(), serde_json::json!(res.source));
                            }
                        }
                        docs.push(doc);
                    }
                    docs
                } else {
                    memory
                        .search_filtered(&req.query, limit, filter_ref)
                        .await
                        .unwrap_or_default()
                }
            } else {
                memory
                    .search_filtered(&req.query, limit, filter_ref)
                    .await
                    .unwrap_or_default()
            };

        let mut documents = Vec::with_capacity(raw_docs.len());
        for doc in raw_docs {
            let kind = doc
                .metadata
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let snip = snippet::clip_chars(&doc.content, 100).to_string();
            let source = doc
                .metadata
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("search_filtered")
                .to_string();

            documents.push(SearchResultItem {
                id: doc.id.unwrap_or_default(),
                path: doc.path,
                content: doc.content,
                score: doc.score,
                source,
                snippet: snip,
                kind,
                metadata: doc.metadata,
                vector_score: None,
                lexical_score: None,
                embedding: doc.content_vector.or(if doc.embedding.is_empty() {
                    None
                } else {
                    Some(doc.embedding)
                }),
            });
        }

        let documents = if req.depth > 0 {
            let docs_to_expand: Vec<MemoryDocument> = documents
                .iter()
                .map(|item| MemoryDocument {
                    id: Some(item.id.clone()),
                    path: item.path.clone(),
                    content: item.content.clone(),
                    metadata: item.metadata.clone(),
                    score: item.score,
                    embedding: item.embedding.clone().unwrap_or_default(),
                    ..MemoryDocument::default()
                })
                .collect();

            let expanded = memory
                .expand_depth(&docs_to_expand, req.depth, filter_ref)
                .await?;
            expanded
                .into_iter()
                .map(|doc| {
                    let kind = doc
                        .metadata
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let snip = snippet::clip_chars(&doc.content, 100).to_string();
                    SearchResultItem {
                        id: doc.id.unwrap_or_default(),
                        path: doc.path,
                        content: doc.content,
                        score: doc.score,
                        source: "expanded".to_string(),
                        snippet: snip,
                        kind,
                        metadata: doc.metadata,
                        vector_score: None,
                        lexical_score: None,
                        embedding: None,
                    }
                })
                .collect()
        } else {
            documents
        };

        let query_vector = if req.include_embedding.unwrap_or(false) {
            match crate::embedding::build_embedder_from_env().await {
                Ok(embedder) => crate::embedding::Embedder::encode(embedder.as_ref(), &req.query)
                    .await
                    .ok()
                    .filter(|vector| !vector.is_empty()),
                Err(_) => None,
            }
        } else {
            None
        };

        let total_available = memory.count().await.unwrap_or(0);

        Ok(SearchResults {
            results: documents,
            query_vector,
            total_available,
            search_type: "hybrid".to_string(),
        })
    }

    /// Build structured context output for specified `ids` or a `query`.
    pub async fn context(
        &self,
        memory: &QmdMemory,
        params: ContextParams,
    ) -> anyhow::Result<MemoryContext> {
        let mut results = Vec::new();

        if let Some(docs) = params.explicit_docs {
            results = docs;
        } else if let Some(ids) = &params.ids {
            for id in ids {
                if let Ok(Some(doc)) = memory.get(id).await {
                    results.push(doc);
                }
            }
        } else if let Some(q) = &params.query {
            results = memory
                .search_filtered(q, params.limit, params.filters.as_ref())
                .await?;
        }

        if results.is_empty() {
            return Ok(MemoryContext {
                total_chars: 0,
                total_records: 0,
                truncated: false,
                truncated_reason: None,
                content: "No relevant context found for query/ids".to_string(),
                sources: Vec::new(),
                estimated_tokens: 0,
            });
        }

        let expanded = if params.depth > 0 {
            memory
                .expand_depth(&results, params.depth, params.filters.as_ref())
                .await?
        } else {
            results
        };

        let mut sources = Vec::new();
        let mut context_str = String::from("# Relevant Memory Context\n\n");
        let mut any_doc_truncated = false;

        for record in &expanded {
            let total_record_chars = record.content.chars().count();
            let is_this_doc_truncated = total_record_chars > params.max_chars_per_doc;
            if is_this_doc_truncated {
                any_doc_truncated = true;
            }

            let doc_content = if is_this_doc_truncated {
                let mut truncated: String =
                    snippet::clip_chars(&record.content, params.max_chars_per_doc).to_string();
                truncated.push_str("\n[... doc truncated ...]");
                truncated
            } else {
                record.content.clone()
            };

            context_str.push_str(&format!(
                "### {} (id: {})\n{}\n\n",
                record.path,
                record.id.as_deref().unwrap_or("none"),
                doc_content
            ));

            let mut meta = record.metadata.clone();
            if let Some(obj) = meta.as_object_mut() {
                obj.insert(
                    "truncated".to_string(),
                    serde_json::json!(is_this_doc_truncated),
                );
                obj.insert(
                    "total_chars".to_string(),
                    serde_json::json!(total_record_chars),
                );
            }

            let kind = record
                .metadata
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            sources.push(SearchResultItem {
                id: record.id.clone().unwrap_or_default(),
                path: record.path.clone(),
                content: record.content.clone(),
                score: record.score,
                source: "search_filtered".to_string(),
                snippet: snippet::clip_chars(&record.content, 200).to_string(),
                kind,
                metadata: meta,
                vector_score: None,
                lexical_score: None,
                embedding: None,
            });
        }

        let mut truncated = any_doc_truncated;
        let mut truncated_reason = None;
        let total_chars = context_str.chars().count();

        if total_chars > params.max_chars {
            truncated = true;
            truncated_reason = Some(format!(
                "Context truncated from {} to {} characters",
                total_chars, params.max_chars
            ));
            let mut truncated_text: String =
                snippet::clip_chars(&context_str, params.max_chars).to_string();
            truncated_text.push_str("\n[... truncated ...]");
            context_str = truncated_text;
        } else if any_doc_truncated {
            truncated_reason = Some("One or more documents were truncated".to_string());
        }

        let final_total_chars = context_str.chars().count();
        let estimated_tokens = crate::context::estimate_tokens(&context_str);

        Ok(MemoryContext {
            total_chars: final_total_chars,
            total_records: expanded.len(),
            truncated,
            truncated_reason,
            content: context_str,
            sources,
            estimated_tokens,
        })
    }
}
