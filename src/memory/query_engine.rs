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
    /// Records considered but not rendered because of `max_chars` (`total_records - sources.len()`).
    #[serde(default)]
    pub omitted_sources: usize,
    pub content: String,
    pub sources: Vec<SearchResultItem>,
    pub estimated_tokens: usize,
}

const CONTEXT_TITLE: &str = "# Relevant Memory Context\n\n";
const CONTEXT_TRUNCATED_MARKER: &str = "\n[... truncated ...]";
const DOC_TRUNCATED_MARKER: &str = "\n[... doc truncated ...]";
/// Trailing "\n\n" after each doc block.
const BLOCK_SEPARATOR_CHARS: usize = 2;

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
            let message = "No relevant context found for query/ids";
            let content = snippet::clip_chars(message, params.max_chars).to_string();
            let total_chars = content.chars().count();
            let truncated = total_chars < message.chars().count();
            let estimated_tokens = crate::domain::cycle_breaks::w30_08::estimate_tokens(&content);
            return Ok(MemoryContext {
                total_chars,
                total_records: 0,
                truncated,
                truncated_reason: truncated.then(|| {
                    format!("Empty context message truncated to {total_chars} characters")
                }),
                omitted_sources: 0,
                content,
                sources: Vec::new(),
                estimated_tokens,
            });
        }

        let expanded = if params.depth > 0 {
            memory
                .expand_depth(&results, params.depth, params.filters.as_ref())
                .await?
        } else {
            results
        };

        // Prepare every block (header + body) so the budget can be applied on whole blocks.
        struct Block<'a> {
            record: &'a MemoryDocument,
            header: String,
            body: String,
            doc_truncated: bool,
            total_record_chars: usize,
        }
        let blocks: Vec<Block<'_>> = expanded
            .iter()
            .map(|record| {
                let total_record_chars = record.content.chars().count();
                let doc_truncated = total_record_chars > params.max_chars_per_doc;
                let body = if doc_truncated {
                    let mut clipped: String =
                        snippet::clip_chars(&record.content, params.max_chars_per_doc).to_string();
                    clipped.push_str(DOC_TRUNCATED_MARKER);
                    clipped
                } else {
                    record.content.clone()
                };
                Block {
                    record,
                    header: format!(
                        "### {} (id: {})\n",
                        record.path,
                        record.id.as_deref().unwrap_or("none")
                    ),
                    body,
                    doc_truncated,
                    total_record_chars,
                }
            })
            .collect();

        let title_chars = CONTEXT_TITLE.chars().count();
        let full_chars = title_chars
            + blocks
                .iter()
                .map(|b| b.header.chars().count() + b.body.chars().count() + BLOCK_SEPARATOR_CHARS)
                .sum::<usize>();
        let marker_chars = CONTEXT_TRUNCATED_MARKER.chars().count();
        let over_budget = full_chars > params.max_chars;

        let mut context_str = String::new();
        // (block index, clipped by the budget)
        let mut rendered: Vec<(usize, bool)> = Vec::new();
        if !over_budget {
            context_str.push_str(CONTEXT_TITLE);
            for (i, b) in blocks.iter().enumerate() {
                context_str.push_str(&b.header);
                context_str.push_str(&b.body);
                context_str.push_str("\n\n");
                rendered.push((i, false));
            }
        } else if params.max_chars >= marker_chars {
            let available = params.max_chars - marker_chars;
            if title_chars <= available {
                context_str.push_str(CONTEXT_TITLE);
                let mut remaining = available - title_chars;
                for (i, b) in blocks.iter().enumerate() {
                    let header_chars = b.header.chars().count();
                    let body_chars = b.body.chars().count();
                    let min_body = body_chars.min(1);
                    // Never emit a header without at least one body char (when it has one).
                    if header_chars + min_body + BLOCK_SEPARATOR_CHARS > remaining {
                        break;
                    }
                    let room = remaining - header_chars - BLOCK_SEPARATOR_CHARS;
                    let clipped = body_chars > room;
                    context_str.push_str(&b.header);
                    context_str.push_str(snippet::clip_chars(&b.body, room));
                    context_str.push_str("\n\n");
                    remaining -= header_chars + body_chars.min(room) + BLOCK_SEPARATOR_CHARS;
                    rendered.push((i, clipped));
                    if clipped {
                        break;
                    }
                }
            }
            context_str.push_str(CONTEXT_TRUNCATED_MARKER);
        }
        // else: budget cannot even hold the marker, so the text is empty and lists no sources.

        let mut sources = Vec::with_capacity(rendered.len());
        let mut any_doc_truncated = false;
        for (i, clipped_by_budget) in rendered {
            let b = &blocks[i];
            let record = b.record;
            let is_truncated = b.doc_truncated || clipped_by_budget;
            any_doc_truncated |= is_truncated;

            let mut meta = record.metadata.clone();
            if let Some(obj) = meta.as_object_mut() {
                obj.insert("truncated".to_string(), serde_json::json!(is_truncated));
                obj.insert(
                    "total_chars".to_string(),
                    serde_json::json!(b.total_record_chars),
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

        let omitted_sources = expanded.len() - sources.len();
        let truncated = over_budget || any_doc_truncated;
        let truncated_reason = if over_budget {
            Some(format!(
                "Context truncated from {} to {} characters; {} source(s) omitted",
                full_chars, params.max_chars, omitted_sources
            ))
        } else if any_doc_truncated {
            Some("One or more documents were truncated".to_string())
        } else {
            None
        };

        let final_total_chars = context_str.chars().count();
        let estimated_tokens = crate::domain::cycle_breaks::w30_08::estimate_tokens(&context_str);

        Ok(MemoryContext {
            total_chars: final_total_chars,
            total_records: expanded.len(),
            truncated,
            truncated_reason,
            omitted_sources,
            content: context_str,
            sources,
            estimated_tokens,
        })
    }
}

#[cfg(test)]
mod context_budget_tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::RwLock as AsyncRwLock;

    fn doc(id: &str, path: &str, content: &str) -> MemoryDocument {
        MemoryDocument {
            id: Some(id.to_string()),
            path: path.to_string(),
            content: content.to_string(),
            score: 1.0,
            metadata: serde_json::json!({}),
            ..Default::default()
        }
    }

    async fn run(docs: Vec<MemoryDocument>, max_chars: usize) -> MemoryContext {
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())));
        MemoryQueryEngine::new()
            .context(
                &memory,
                ContextParams {
                    explicit_docs: Some(docs),
                    limit: 10,
                    max_chars,
                    max_chars_per_doc: 10_000,
                    ..Default::default()
                },
            )
            .await
            .expect("context")
    }

    /// Core invariants shared by every budget: bounded, aligned, no partial header.
    fn assert_invariants(ctx: &MemoryContext, docs: &[MemoryDocument], max_chars: usize) {
        let chars = ctx.content.chars().count();
        assert!(
            chars <= max_chars,
            "{chars} > {max_chars}: {:?}",
            ctx.content
        );
        assert_eq!(ctx.total_chars, chars);
        let headers = ctx.content.matches("### ").count();
        assert_eq!(headers, ctx.sources.len(), "headers vs sources");
        for (src, d) in ctx.sources.iter().zip(docs) {
            let header = format!("### {} (id: {})\n", d.path, d.id.as_deref().unwrap());
            assert!(
                ctx.content.contains(&header),
                "full header missing: {header:?}"
            );
            assert_eq!(src.id, d.id.clone().unwrap());
        }
        assert_eq!(ctx.omitted_sources, ctx.total_records - ctx.sources.len());
    }

    #[tokio::test]
    async fn empty_context_obeys_budget_and_reports_actual_size() {
        for max_chars in [0usize, 1, 32, 38, 39, 60, 200] {
            let ctx = run(Vec::new(), max_chars).await;
            assert_invariants(&ctx, &[], max_chars);
            assert_eq!(ctx.total_records, 0);
            assert_eq!(ctx.omitted_sources, 0);
            assert_eq!(
                ctx.content,
                snippet::clip_chars("No relevant context found for query/ids", max_chars)
            );
            assert_eq!(ctx.truncated, max_chars < 39);
        }
    }

    #[tokio::test]
    async fn tiny_budgets_never_exceed_max_or_emit_partial_headers() {
        let docs = vec![doc("a1", "docs/a", &"x".repeat(500))];
        for max_chars in [0usize, 1, 32, 60, 200] {
            let ctx = run(docs.clone(), max_chars).await;
            assert_invariants(&ctx, &docs, max_chars);
            assert!(ctx.truncated, "max_chars={max_chars}");
            assert_eq!(ctx.total_records, 1);
        }
    }

    #[tokio::test]
    async fn budget_below_marker_is_empty_without_sources() {
        let docs = vec![doc("a1", "docs/a", "body")];
        for max_chars in [0usize, 1] {
            let ctx = run(docs.clone(), max_chars).await;
            assert_eq!(ctx.content, "");
            assert!(ctx.sources.is_empty());
            assert_eq!(ctx.omitted_sources, 1);
            assert!(ctx.truncated);
        }
        // 32 holds the marker but not the title + a full header: marker only, no sources.
        let ctx = run(docs.clone(), 32).await;
        assert_eq!(ctx.content, CONTEXT_TRUNCATED_MARKER);
        assert!(ctx.sources.is_empty());
    }

    #[tokio::test]
    async fn marker_is_counted_inside_the_budget() {
        let docs = vec![doc("a1", "docs/a", &"y".repeat(500))];
        let ctx = run(docs.clone(), 100).await;
        assert_eq!(
            ctx.content.chars().count(),
            100,
            "budget fully used incl. marker"
        );
        assert!(ctx.content.ends_with(CONTEXT_TRUNCATED_MARKER));
        assert_eq!(ctx.sources.len(), 1);
        assert_eq!(
            ctx.sources[0].metadata["truncated"],
            serde_json::json!(true)
        );
        assert_invariants(&ctx, &docs, 100);
    }

    #[tokio::test]
    async fn omitted_second_doc_is_not_listed_as_source() {
        let docs = vec![
            doc("a1", "docs/a", "short"),
            doc("b2", "docs/b", &"z".repeat(400)),
        ];
        // title(27) + block a(27) + marker(20) = 74 fits; b needs header(20)+1+2 more, only 16 left at 90
        let ctx = run(docs.clone(), 90).await;
        assert_invariants(&ctx, &docs[..1], 90);
        assert_eq!(ctx.sources.len(), 1);
        assert_eq!(ctx.sources[0].id, "a1");
        assert_eq!(ctx.omitted_sources, 1);
        assert_eq!(ctx.total_records, 2);
        assert!(!ctx.content.contains("docs/b"));
        assert!(ctx.truncated);
        assert!(ctx
            .truncated_reason
            .as_deref()
            .unwrap()
            .contains("1 source(s) omitted"));
    }

    #[tokio::test]
    async fn unicode_is_counted_in_chars_not_bytes() {
        let docs = vec![doc("u1", "docs/ñandú", &"日本語é".repeat(100))];
        for max_chars in [32usize, 60, 90, 200] {
            let ctx = run(docs.clone(), max_chars).await;
            assert_invariants(&ctx, &docs, max_chars);
            assert!(ctx.content.chars().count() < ctx.content.len() || ctx.sources.is_empty());
        }
    }

    #[tokio::test]
    async fn ample_budget_keeps_full_untruncated_output() {
        let docs = vec![doc("a1", "docs/a", "alpha"), doc("b2", "docs/b", "beta")];
        let ctx = run(docs.clone(), 10_000).await;
        assert_eq!(
            ctx.content,
            "# Relevant Memory Context\n\n### docs/a (id: a1)\nalpha\n\n### docs/b (id: b2)\nbeta\n\n"
        );
        assert!(!ctx.truncated);
        assert!(ctx.truncated_reason.is_none());
        assert_eq!(ctx.sources.len(), 2);
        assert_eq!(ctx.omitted_sources, 0);
        assert_eq!(ctx.total_chars, ctx.content.chars().count());
        // exact fit is not truncated
        let exact = ctx.content.chars().count();
        let ctx = run(docs, exact).await;
        assert!(!ctx.truncated);
        assert_eq!(ctx.sources.len(), 2);
    }
}
