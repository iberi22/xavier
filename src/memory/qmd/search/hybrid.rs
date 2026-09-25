//! Hybrid search combining keyword (lexical) and vector retrieval.
//!
//! Uses reciprocal rank fusion (RRF) to merge keyword and vector results,
//! plus multi-hop context expansion and re-ranking.

use std::collections::HashMap;

use anyhow::Result;
use autometrics::autometrics;

use crate::memory::qmd_memory::config::*;
use crate::memory::qmd_memory::query_builder;
use crate::memory::qmd_memory::query_builder::{extract_candidate_terms_internal, normalize_query};
use crate::memory::qmd_memory::types::MemoryDocument;
use crate::memory::qmd_memory::utils::*;
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::schema::MemoryQueryFilters;

use super::scoring::{contextual_boost, lexical_score};
use super::vector::{vsearch, vsearch_filtered};

/// Hybrid search with variant expansion, multi-hop context, and RRF re-ranking.
///
/// Thin wrapper over [`search_hybrid_optimized_with_mode`] that drops the
/// `vector_used` flag, kept for the many call sites that only need the
/// documents. Prefer the `_with_mode` variant when the caller wants to
/// surface "hybrid" vs "lexical-only" degradation to a client/response.
#[autometrics]
pub async fn search_hybrid_optimized(
    memory: &QmdMemory,
    query_text: &str,
    limit: usize,
    filters: Option<&MemoryQueryFilters>,
) -> Result<Vec<MemoryDocument>> {
    search_hybrid_optimized_with_mode(memory, query_text, limit, filters)
        .await
        .map(|(docs, _vector_used)| docs)
}

/// Same as [`search_hybrid_optimized`] but also reports whether the vector
/// (embedding) signal actually contributed to the result set. `false` means
/// the result is lexical/FTS-only, either because no embedder is configured
/// or because the embedding call failed/timed out and search degraded
/// gracefully instead of hanging.
#[autometrics]
pub async fn search_hybrid_optimized_with_mode(
    memory: &QmdMemory,
    query_text: &str,
    limit: usize,
    filters: Option<&MemoryQueryFilters>,
) -> Result<(Vec<MemoryDocument>, bool)> {
    let query_bundle = query_builder::build_query_bundle_internal(query_text);
    let mut candidate_scores: HashMap<String, (f32, MemoryDocument, f32)> = HashMap::new();
    let mut vector_used = false;

    // 1. Lexical retrieval across query variants
    for expanded_query in &query_bundle.variants {
        let cache_hit = memory
            .search_with_cache_filtered(expanded_query, limit.max(3), filters)
            .await?;
        merge_ranked_candidates(
            &mut candidate_scores,
            cache_hit.documents,
            expanded_query,
            query_bundle.weight_for(expanded_query) * KEYWORD_WEIGHT,
        );
    }

    // 2. Vector retrieval (when embedder is configured). Bounded by a short,
    // configurable fallback budget (`XAVIER_EMBEDDING_FALLBACK_BUDGET_MS`,
    // default 2s): on a fresh install with an unreachable embedding
    // endpoint, `generate_embedding` alone can retry for 10s+ internally.
    // Without this outer timeout that unbounded wait used to make
    // `mem_search`/`memory_search` hang well past client/middleware
    // timeouts instead of degrading to the lexical results already
    // gathered above.
    if crate::memory::embedder::EmbeddingClient::is_configured_from_env()
        || std::env::var("XAVIER_EMBEDDING_URL").is_ok()
    {
        let embed_budget = crate::memory::embedder::embedding_fallback_budget();
        match tokio::time::timeout(
            embed_budget,
            crate::memory::qmd_memory::reader::generate_embedding(&query_bundle.normalized_query),
        )
        .await
        {
            Ok(Ok(vector)) if !vector.is_empty() => {
                if let Ok(filtered_hits) =
                    vsearch_filtered(memory, vector, limit.max(5), filters).await
                {
                    vector_used = true;
                    merge_ranked_candidates(
                        &mut candidate_scores,
                        filtered_hits,
                        &query_bundle.normalized_query,
                        SEMANTIC_WEIGHT,
                    );
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                tracing::warn!(
                    error = %error,
                    "hybrid search: embedding generation failed, degrading to lexical-only"
                );
            }
            Err(_) => {
                tracing::warn!(
                    budget_ms = embed_budget.as_millis() as u64,
                    "hybrid search: embedding generation timed out, degrading to lexical-only"
                );
            }
        }
    }

    if candidate_scores.is_empty() {
        return Ok((Vec::new(), vector_used));
    }

    let mut candidates: Vec<(f32, MemoryDocument, f32)> =
        candidate_scores.values().cloned().collect();
    candidates.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.path.cmp(&right.1.path))
    });

    let seed_docs: Vec<MemoryDocument> = candidates
        .iter()
        .take(limit.max(3))
        .map(|(_, doc, _)| doc.clone())
        .collect();

    let multi_hop_docs = memory
        .multi_hop_context(query_text, &seed_docs, filters)
        .await;

    for doc in multi_hop_docs {
        let score = contextual_boost(&query_bundle.normalized_query, &doc, 0.45);
        candidate_scores
            .entry(doc.id.clone().unwrap_or_else(|| doc.path.clone()))
            .and_modify(|entry| entry.0 += score)
            .or_insert((score, doc, 0.45));
    }

    let mut reranked: Vec<(f32, MemoryDocument, f32)> =
        candidate_scores.values().cloned().collect();
    reranked.truncate(MAX_RERANK_CANDIDATES.max(limit));
    reranked.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                right
                    .2
                    .partial_cmp(&left.2)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.1.path.cmp(&right.1.path))
    });

    Ok((
        reranked
            .into_iter()
            .take(limit)
            .map(|(score, mut doc, _)| {
                doc.score = score;
                doc
            })
            .collect(),
        vector_used,
    ))
}

/// Merge ranked candidates using RRF + contextual boost + temporal decay.
pub fn merge_ranked_candidates(
    candidate_scores: &mut HashMap<String, (f32, MemoryDocument, f32)>,
    documents: Vec<MemoryDocument>,
    query: &str,
    query_weight: f32,
) {
    let half_life_days = std::env::var("XAVIER_TEMPORAL_HALF_LIFE_DAYS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(30.0);

    for (rank, doc) in documents.into_iter().enumerate() {
        let key = doc.id.clone().unwrap_or_else(|| doc.path.clone());
        let rrf_score = 1.0 / (RRF_K + (rank as f32) + 1.0);
        let rerank = contextual_boost(query, &doc, query_weight);
        let created_at = doc
            .metadata
            .get("created_at")
            .or_else(|| doc.metadata.get("timestamp"))
            .or_else(|| doc.metadata.get("updated_at"))
            .and_then(|v| v.as_str());
        let decay = if is_locomo_document(&doc.path, &doc.metadata) {
            1.0
        } else {
            super::scoring::temporal_decay_multiplier(created_at, half_life_days)
        };
        let combined = ((rrf_score * query_weight) + rerank) * decay;
        candidate_scores
            .entry(key)
            .and_modify(|entry| {
                entry.0 += combined;
                entry.2 = entry.2.max(query_weight);
            })
            .or_insert((combined, doc, query_weight));
    }
}

/// Hybrid search combining keyword BM25 and vector cosine similarity via RRF.
pub async fn query_with_hybrid_search(
    memory: &QmdMemory,
    query_text: &str,
    query_vector: Vec<f32>,
    limit: usize,
) -> Result<Vec<MemoryDocument>> {
    let mut scores: HashMap<String, (f32, MemoryDocument)> = HashMap::new();

    let keyword_hits = memory
        .search_with_cache_filtered(query_text, limit, None)
        .await?;
    for (rank, doc) in keyword_hits.documents.into_iter().enumerate() {
        let key = doc
            .id
            .clone()
            .unwrap_or_else(|| format!("path:{}", doc.path));
        let rrf_score = 1.0 / (RRF_K + rank as f32 + 1.0);
        scores.insert(key, (rrf_score * KEYWORD_WEIGHT, doc));
    }

    let vector_hits = vsearch(memory, query_vector, limit).await?;
    for (rank, doc) in vector_hits.into_iter().enumerate() {
        let key = doc
            .id
            .clone()
            .unwrap_or_else(|| format!("path:{}", doc.path));
        let rrf_score = 1.0 / (RRF_K + rank as f32 + 1.0);
        if let Some((existing, _)) = scores.get_mut(&key) {
            *existing += rrf_score * SEMANTIC_WEIGHT;
        } else {
            scores.insert(key, (rrf_score * SEMANTIC_WEIGHT, doc));
        }
    }

    let mut fused: Vec<(f32, MemoryDocument)> = scores.into_values().collect();
    fused.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.path.cmp(&b.1.path))
    });

    Ok(fused
        .into_iter()
        .map(|(score, mut doc)| {
            doc.score = score;
            doc
        })
        .take(limit)
        .collect())
}

/// Filtered search combining keyword and vector results.
pub async fn query_filtered(
    memory: &QmdMemory,
    query_text: &str,
    query_vector: Vec<f32>,
    limit: usize,
    filters: Option<&MemoryQueryFilters>,
) -> Result<Vec<MemoryDocument>> {
    let mut keyword_results = memory
        .search_with_cache_filtered(query_text, limit, filters)
        .await?
        .documents;

    let locomo_only = !keyword_results.is_empty()
        && keyword_results
            .iter()
            .all(|doc| is_locomo_document(&doc.path, &doc.metadata));

    let mut expanded_terms = Vec::new();
    let expansion_seed = if locomo_only {
        keyword_results
            .iter()
            .find(|doc| {
                doc.metadata.get("category").and_then(|v| v.as_str()) != Some("session_summary")
            })
            .or_else(|| keyword_results.first())
    } else {
        keyword_results.first()
    };

    if let Some(top_doc) = expansion_seed {
        let query_lower = query_text.to_lowercase();
        for w in top_doc.content.split_whitespace() {
            let w_clean = w
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if w_clean.len() >= 3 && !query_lower.contains(&w_clean) {
                expanded_terms.push(w_clean);
            }
        }
        expanded_terms.truncate(5);
    }

    for entity in expanded_terms {
        if let Ok(expanded) = memory.search_with_cache_filtered(&entity, 2, filters).await {
            for doc in expanded.documents {
                if keyword_results.len() > 1 {
                    keyword_results.insert(1, doc);
                } else {
                    keyword_results.push(doc);
                }
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    keyword_results.retain(|doc| {
        let key = doc.id.clone().unwrap_or_else(|| doc.path.clone());
        seen.insert(key)
    });

    let vector_results = if query_vector.is_empty() {
        Vec::new()
    } else {
        vsearch_filtered(memory, query_vector.clone(), limit, filters)
            .await
            .unwrap_or_default()
    };

    if vector_results.is_empty() {
        if keyword_results.is_empty() {
            // Diagnostic: both stages missed. Log what the query looked like
            // against the live index so empty-search reports are actionable
            // (lexical miss vs vector miss vs over-filtering).
            let indexed_docs = memory.docs.read().await.len();
            tracing::debug!(
                query = %query_text,
                limit = limit,
                indexed_docs = indexed_docs,
                query_vector_empty = query_vector.is_empty(),
                has_filters = filters.is_some(),
                "hybrid search produced zero candidates"
            );
        }
        return Ok(keyword_results.into_iter().take(limit).collect());
    }

    let mut scores: HashMap<String, (f32, MemoryDocument)> = HashMap::new();

    for (rank, doc) in keyword_results.into_iter().enumerate() {
        let key = doc
            .id
            .clone()
            .unwrap_or_else(|| format!("path:{}", doc.path));
        let rrf_score = 1.0 / (RRF_K + rank as f32 + 1.0);
        scores.insert(key, (rrf_score * KEYWORD_WEIGHT, doc));
    }

    for (rank, doc) in vector_results.into_iter().enumerate() {
        let key = doc
            .id
            .clone()
            .unwrap_or_else(|| format!("path:{}", doc.path));
        let rrf_score = 1.0 / (RRF_K + rank as f32 + 1.0);
        if let Some((existing, _)) = scores.get_mut(&key) {
            *existing += rrf_score * SEMANTIC_WEIGHT;
        } else {
            scores.insert(key, (rrf_score * SEMANTIC_WEIGHT, doc));
        }
    }

    let mut fused: Vec<(f32, MemoryDocument)> = scores.into_values().collect();
    fused.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    Ok(fused
        .into_iter()
        .map(|(score, mut d)| {
            d.score = score;
            d
        })
        .take(limit)
        .collect())
}

/// BM25-style lexical search over all documents.
pub async fn bm25_search(
    memory: &QmdMemory,
    query: &str,
    limit: usize,
) -> Result<Vec<MemoryDocument>> {
    let normalized = normalize_query(query);
    let docs = memory.docs.read().await;

    let mut scores: Vec<(f32, MemoryDocument)> = docs
        .iter()
        .filter_map(|doc| {
            let score = lexical_score(doc, &normalized);
            if score > 0.0 {
                let mut d = doc.clone();
                d.score = score;
                Some((score, d))
            } else {
                None
            }
        })
        .collect();

    scores.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.path.cmp(&b.1.path))
    });

    Ok(scores.into_iter().take(limit).map(|(_, d)| d).collect())
}

/// Expand context by finding related documents from seed documents.
pub async fn multi_hop_context(
    memory: &QmdMemory,
    query_text: &str,
    seed_docs: &[MemoryDocument],
    filters: Option<&MemoryQueryFilters>,
) -> Vec<MemoryDocument> {
    let mut expanded = Vec::new();
    let query_terms = normalize_query(query_text);

    for doc in seed_docs.iter().take(MAX_MULTI_HOP_DEPTH) {
        let mut extracted = extract_candidate_terms_internal(&doc.content);
        extracted.extend(extract_candidate_terms_internal(&doc.path));
        extracted.sort();
        extracted.dedup();
        for term in extracted.into_iter().take(MAX_EXPANSIONS) {
            if query_terms.contains(&term) {
                continue;
            }
            if let Ok(results) = memory.search_with_cache_filtered(&term, 2, filters).await {
                expanded.extend(results.documents);
            }
        }
    }

    expanded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::RwLock as AsyncRwLock;

    const TEST_DIM: usize = 8;

    fn test_doc(path: &str, content: &str) -> MemoryDocument {
        MemoryDocument {
            id: Some(path.to_string()),
            path: path.to_string(),
            content: content.to_string(),
            metadata: serde_json::json!({}),
            ..Default::default()
        }
    }

    /// Minimal HTTP stub: fast on every route except POST /api/embed, which
    /// sleeps `slow_secs` before responding — simulates the fresh-install
    /// scenario in issue #2544 where the configured embedding endpoint is
    /// reachable but never answers in time.
    async fn run_slow_embed_server(slow_secs: u64) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test embed server");
        let addr = listener.local_addr().expect("test server addr");
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut chunk = vec![0u8; 8192];
                    let mut req = Vec::new();
                    loop {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => {
                                req.extend_from_slice(&chunk[..n]);
                                if req.windows(4).any(|w| w == b"\r\n\r\n") {
                                    break;
                                }
                                if req.len() > 65536 {
                                    return;
                                }
                            }
                        }
                    }
                    let head_end = req.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(0) + 4;
                    let head = String::from_utf8_lossy(&req[..head_end]).to_string();
                    let content_len: usize = head
                        .lines()
                        .filter_map(|line| {
                            let (k, v) = line.split_once(':')?;
                            if k.trim().eq_ignore_ascii_case("content-length") {
                                v.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                        .next()
                        .unwrap_or(0);
                    let mut body = req[head_end.min(req.len())..].to_vec();
                    while body.len() < content_len {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => body.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let (payload, status) = if head.starts_with("GET /v1/models") {
                        (
                            r#"{"object":"list","data":[{"id":"test-embed","object":"model"}]}"#
                                .to_string(),
                            200,
                        )
                    } else if head.starts_with("POST /api/embed") {
                        tokio::time::sleep(Duration::from_secs(slow_secs)).await;
                        let vec_json = ["0.5"; TEST_DIM].join(",");
                        (
                            format!(r#"{{"model":"test-embed","embeddings":[[{vec_json}]]}}"#),
                            200,
                        )
                    } else {
                        (r#"{"error":"not found"}"#.to_string(), 404)
                    };
                    let resp = format!(
                        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                        payload.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    /// Regression for issue #2544: `search_hybrid_optimized` used to call
    /// `generate_embedding` for the query vector with no outer timeout, so a
    /// configured-but-unreachable/hanging embedding endpoint made
    /// `mem_search`/`memory_search` hang for 15s+ (3 retries x 5s) instead of
    /// falling back to the lexical results already gathered. It must now
    /// degrade to lexical-only within the configured fallback budget.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::await_holding_lock)]
    async fn test_search_hybrid_optimized_degrades_to_lexical_on_embedder_timeout() {
        let _temp_env = crate::settings::tests::TempEnv::new();
        for key in [
            "XAVIER_EMBEDDING_PROVIDER_MODE",
            "XAVIER_EMBED_PROVIDER",
            "XAVIER_EMBEDDER",
            "XAVIER_EMBEDDING_URL",
            "XAVIER_EMBEDDING_LOCAL_URL",
            "XAVIER_EMBEDDING_MODEL",
            "XAVIER_OLLAMA_MODEL",
            "XAVIER_OLLAMA_URL",
            "XAVIER_OLLAMA_DIMS",
            "XAVIER_EMBEDDING_FALLBACK_BUDGET_MS",
            "OPENAI_API_KEY",
            "XAVIER_EMBEDDING_API_KEY",
            "XAVIER_EMBEDDING_CLOUD_MODEL",
        ] {
            std::env::remove_var(key);
        }

        // Embed endpoint sleeps 5s on every call; the fallback budget below
        // is far shorter, so the outer timeout must win.
        let base = run_slow_embed_server(5).await;
        std::env::set_var("XAVIER_OLLAMA_URL", format!("{base}/api/embed"));
        std::env::set_var("XAVIER_OLLAMA_MODEL", "test-embed");
        std::env::set_var("XAVIER_OLLAMA_DIMS", TEST_DIM.to_string());
        std::env::set_var("_XAVIER_TEST_OLLAMA_PROBE_URL", format!("{base}/v1/models"));
        std::env::set_var("XAVIER_EMBEDDING_FALLBACK_BUDGET_MS", "300");

        let docs = vec![test_doc(
            "notes/alpha",
            "alpha cluster registration workflow ledger",
        )];
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(docs)));

        let start = Instant::now();
        let (results, vector_used) =
            search_hybrid_optimized_with_mode(&memory, "alpha registration", 5, None)
                .await
                .expect("hybrid search must not error when the embedder hangs");
        let elapsed = start.elapsed();

        std::env::remove_var("_XAVIER_TEST_OLLAMA_PROBE_URL");

        assert!(
            !vector_used,
            "vector signal must not be reported as used when the embedder times out"
        );
        assert!(
            !results.is_empty(),
            "lexical results must still be returned when the embedder hangs"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "search must degrade to lexical within ~2s (300ms budget), took {elapsed:?}"
        );
    }

    #[test]
    fn test_merge_ranked_candidates_rrf_and_decay() {
        let mut candidate_scores: HashMap<String, (f32, MemoryDocument, f32)> = HashMap::new();

        let fresh_now = chrono::Utc::now().to_rfc3339();
        let stale_old = (chrono::Utc::now() - chrono::Duration::days(120)).to_rfc3339();

        let doc_fresh = MemoryDocument {
            id: Some("doc_fresh".to_string()),
            path: "notes/rust_performance.md".to_string(),
            content: "Rust memory management and async runtime optimization".to_string(),
            metadata: serde_json::json!({
                "created_at": fresh_now
            }),
            ..Default::default()
        };

        let doc_stale = MemoryDocument {
            id: Some("doc_stale".to_string()),
            path: "notes/legacy_notes.md".to_string(),
            content: "Rust memory management and async runtime optimization".to_string(),
            metadata: serde_json::json!({
                "created_at": stale_old
            }),
            ..Default::default()
        };

        // Pass 1: Lexical rank list (both present at rank 0 and 1)
        merge_ranked_candidates(
            &mut candidate_scores,
            vec![doc_stale.clone(), doc_fresh.clone()],
            "rust memory",
            1.0,
        );

        // Pass 2: Vector rank list where doc_fresh is present
        merge_ranked_candidates(
            &mut candidate_scores,
            vec![doc_fresh.clone()],
            "rust memory",
            0.5,
        );

        let fresh_score = candidate_scores.get("doc_fresh").unwrap().0;
        let stale_score = candidate_scores.get("doc_stale").unwrap().0;

        assert!(
            fresh_score > stale_score,
            "Fresh record with dual lexical+vector RRF votes should rank higher than stale record (fresh: {}, stale: {})",
            fresh_score,
            stale_score
        );
    }
}
