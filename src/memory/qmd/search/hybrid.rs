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

/// A single fused search candidate.
///
/// `fused` is the rank score used for ordering; `relevance` is the
/// normalized direct-query match strength in [0,1] used for the relevance
/// floor (multi-hop context-expansion documents carry `0.0`: the query
/// never matched them directly, they only boost already-seen documents).
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub fused: f32,
    pub relevance: f32,
    pub doc: MemoryDocument,
}

/// Which retrieval leg produced a ranked document list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalLeg {
    Lexical,
    Vector,
}

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
    let mut candidate_scores: HashMap<String, ScoredCandidate> = HashMap::new();
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
            RetrievalLeg::Lexical,
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
                        RetrievalLeg::Vector,
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

    let mut candidates: Vec<ScoredCandidate> = candidate_scores.values().cloned().collect();
    candidates.sort_by(|left, right| {
        right
            .fused
            .partial_cmp(&left.fused)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.doc.path.cmp(&right.doc.path))
    });

    let seed_docs: Vec<MemoryDocument> = candidates
        .iter()
        .take(limit.max(3))
        .map(|candidate| candidate.doc.clone())
        .collect();

    let multi_hop_docs = memory
        .multi_hop_context(query_text, &seed_docs, filters)
        .await;

    for doc in multi_hop_docs {
        let score = contextual_boost(&query_bundle.normalized_query, &doc, 0.45);
        candidate_scores
            .entry(doc.id.clone().unwrap_or_else(|| doc.path.clone()))
            .and_modify(|entry| entry.fused += score)
            .or_insert(ScoredCandidate {
                fused: score,
                // The query never matched this document directly: it is
                // context expansion, so it must not pass the relevance floor.
                relevance: 0.0,
                doc,
            });
    }

    let reranked = rank_candidates(candidate_scores.into_values().collect(), limit);

    Ok((
        reranked
            .into_iter()
            .take(limit)
            .map(|candidate| {
                let mut doc = candidate.doc;
                doc.score = candidate.fused;
                doc
            })
            .collect(),
        vector_used,
    ))
}

/// Merge ranked candidates using RRF + contextual boost + temporal decay.
///
/// `signal` is the rank constant (`1/(RRF_K+rank+1)`) for the [`RetrievalLeg::Lexical`]
/// leg — FTS yields an honest rank but no calibrated score — and the real
/// cosine similarity already stored in `doc.score` by `vsearch` (normalized
/// to [0,1]) for the [`RetrievalLeg::Vector`] leg.
pub fn merge_ranked_candidates(
    candidate_scores: &mut HashMap<String, ScoredCandidate>,
    documents: Vec<MemoryDocument>,
    query: &str,
    query_weight: f32,
    leg: RetrievalLeg,
) {
    let half_life_days = std::env::var("XAVIER_TEMPORAL_HALF_LIFE_DAYS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(30.0);

    for (rank, doc) in documents.into_iter().enumerate() {
        let key = doc.id.clone().unwrap_or_else(|| doc.path.clone());
        let rrf_score = 1.0 / (RRF_K + (rank as f32) + 1.0);
        let signal = match leg {
            RetrievalLeg::Lexical => rrf_score,
            RetrievalLeg::Vector => doc.score.clamp(0.0, 1.0),
        };
        // A lexical hit is a direct match: full relevance, so the floor can
        // never drop it (its rank constant is far below MIN_RELEVANCE_SCORE).
        let relevance = match leg {
            RetrievalLeg::Lexical => 1.0,
            RetrievalLeg::Vector => signal,
        };
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
        let combined = ((signal * query_weight) + rerank) * decay;
        candidate_scores
            .entry(key)
            .and_modify(|entry| {
                entry.fused += combined;
                entry.relevance = entry.relevance.max(relevance);
            })
            .or_insert(ScoredCandidate {
                fused: combined,
                relevance,
                doc,
            });
    }
}

/// The single place that orders fused candidates: sort by `fused` desc,
/// then `relevance` desc, then `doc.path` asc (deterministic); cap at
/// `MAX_RERANK_CANDIDATES`; drop anything below the relevance floor unless
/// the floor would empty a non-empty list (then it is not applied).
///
/// Sorting before truncating is the point: truncating first drops the best
/// document whenever it is not among the first values of a HashMap
/// iteration.
pub fn rank_candidates(mut candidates: Vec<ScoredCandidate>, limit: usize) -> Vec<ScoredCandidate> {
    candidates.sort_by(|left, right| {
        right
            .fused
            .partial_cmp(&left.fused)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                right
                    .relevance
                    .partial_cmp(&left.relevance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.doc.path.cmp(&right.doc.path))
    });
    candidates.truncate(MAX_RERANK_CANDIDATES.max(limit));
    if candidates.is_empty() {
        return candidates;
    }
    let kept: Vec<ScoredCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.relevance >= MIN_RELEVANCE_SCORE)
        .cloned()
        .collect();
    if kept.is_empty() {
        candidates
    } else {
        kept
    }
}

/// Hybrid search combining keyword BM25 and vector cosine similarity via RRF.
pub async fn query_with_hybrid_search(
    memory: &QmdMemory,
    query_text: &str,
    query_vector: Vec<f32>,
    limit: usize,
) -> Result<Vec<MemoryDocument>> {
    let mut scores: HashMap<String, ScoredCandidate> = HashMap::new();

    let keyword_hits = memory
        .search_with_cache_filtered(query_text, limit, None)
        .await?;
    for (rank, doc) in keyword_hits.documents.into_iter().enumerate() {
        let key = doc
            .id
            .clone()
            .unwrap_or_else(|| format!("path:{}", doc.path));
        let signal = 1.0 / (RRF_K + rank as f32 + 1.0);
        let fused = signal * KEYWORD_WEIGHT;
        scores
            .entry(key)
            .and_modify(|entry: &mut ScoredCandidate| {
                entry.fused += fused;
                entry.relevance = entry.relevance.max(signal);
            })
            .or_insert(ScoredCandidate {
                fused,
                relevance: signal,
                doc,
            });
    }

    let vector_hits = vsearch(memory, query_vector, limit).await?;
    for doc in vector_hits.into_iter() {
        let key = doc
            .id
            .clone()
            .unwrap_or_else(|| format!("path:{}", doc.path));
        // The real cosine similarity (`vsearch` normalizes it to [0,1]),
        // not the rank constant: a top-ranked vector hit with similarity
        // 1.0 must outweigh a top-ranked lexical hit (0.7/61 vs 0.3/61
        // under rank-only fusion buried the vector leg).
        let signal = doc.score.clamp(0.0, 1.0);
        let fused = signal * SEMANTIC_WEIGHT;
        scores
            .entry(key)
            .and_modify(|entry: &mut ScoredCandidate| {
                entry.fused += fused;
                entry.relevance = entry.relevance.max(signal);
            })
            .or_insert(ScoredCandidate {
                fused,
                relevance: signal,
                doc,
            });
    }

    // NOTE: deliberately no relevance floor here (unlike `rank_candidates`).
    // This entry point has no multi-hop context-expansion leg, so every
    // candidate matched the query directly through at least one leg; the
    // floor exists to drop expansion noise, and a rank-based lexical
    // relevance (max 1/(RRF_K+1)) can never clear it, so flooring would
    // discard genuine direct matches whenever any vector hit survives.
    let mut fused: Vec<ScoredCandidate> = scores.into_values().collect();
    fused.sort_by(|left, right| {
        right
            .fused
            .partial_cmp(&left.fused)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                right
                    .relevance
                    .partial_cmp(&left.relevance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.doc.path.cmp(&right.doc.path))
    });
    fused.truncate(MAX_RERANK_CANDIDATES.max(limit));

    Ok(fused
        .into_iter()
        .map(|candidate| {
            let mut doc = candidate.doc;
            doc.score = candidate.fused;
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
        let mut candidate_scores: HashMap<String, ScoredCandidate> = HashMap::new();

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
            RetrievalLeg::Lexical,
        );

        // Pass 2: Vector rank list where doc_fresh is present
        merge_ranked_candidates(
            &mut candidate_scores,
            vec![doc_fresh.clone()],
            "rust memory",
            0.5,
            RetrievalLeg::Vector,
        );

        let fresh_score = candidate_scores.get("doc_fresh").unwrap().fused;
        let stale_score = candidate_scores.get("doc_stale").unwrap().fused;

        assert!(
            fresh_score > stale_score,
            "Fresh record with dual lexical+vector RRF votes should rank higher than stale record (fresh: {}, stale: {})",
            fresh_score,
            stale_score
        );
    }

    fn scored(path: &str, fused: f32, relevance: f32) -> ScoredCandidate {
        ScoredCandidate {
            fused,
            relevance,
            doc: test_doc(path, "contenido de prueba"),
        }
    }

    /// Corpus doc with an explicit neutral kind: without it,
    /// `infer_kind_from_path` tags every `*.md` path as `File`, which adds
    /// a flat +5.0 lexical bonus on ANY query — an unrelated query would
    /// then always match everything and could never return empty, and a
    /// vector-only doc would also match lexically (masking the vector leg).
    fn neutral_kind_doc(path: &str, content: &str) -> MemoryDocument {
        let mut doc = test_doc(path, content);
        doc.metadata = serde_json::json!({"kind": "document"});
        doc
    }

    /// Regression: candidates were truncated BEFORE sorting, so the best
    /// document was dropped whenever it was not among the first 32 values
    /// of a HashMap iteration. The input is a Vec (deterministic): the
    /// winner sits past the truncation point and must still come first.
    #[test]
    fn rank_candidates_sorts_before_truncating() {
        let mut input = Vec::new();
        for index in 0..39 {
            input.push(scored(
                &format!("notas/doc{index:02}.md"),
                index as f32,
                0.5,
            ));
        }
        input.push(scored("zzz/ultimo.md", 1000.0, 0.5));
        let ranked = rank_candidates(input, 5);
        assert_eq!(ranked[0].doc.path, "zzz/ultimo.md");
        assert_eq!(ranked.len(), MAX_RERANK_CANDIDATES);
    }

    /// The relevance floor drops context-expansion noise (relevance 0.0)
    /// but must never empty a non-empty candidate list.
    #[test]
    fn rank_candidates_drops_zero_relevance_but_never_empties() {
        let mixed = vec![
            scored("notas/ruido.md", 5.0, 0.0),
            scored("notas/bueno.md", 0.1, 0.6),
        ];
        let ranked = rank_candidates(mixed, 5);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].doc.path, "notas/bueno.md");

        let all_zero = vec![scored("notas/solo.md", 5.0, 0.0)];
        let ranked = rank_candidates(all_zero, 5);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].doc.path, "notas/solo.md");
    }

    /// Regression: lexical relevance was the rank constant (<= 1/61), below
    /// the floor, so any vector hit above the floor dropped every
    /// lexical-only match.
    #[test]
    fn relevance_floor_keeps_lexical_only_match_next_to_vector_hit() {
        let mut candidate_scores: HashMap<String, ScoredCandidate> = HashMap::new();
        let lexical = neutral_kind_doc("notas/lexico.md", "codigo de error e1234");
        let mut vector = neutral_kind_doc("notas/vector.md", "texto sin relacion");
        vector.score = 0.9;
        merge_ranked_candidates(
            &mut candidate_scores,
            vec![lexical],
            "e1234",
            KEYWORD_WEIGHT,
            RetrievalLeg::Lexical,
        );
        merge_ranked_candidates(
            &mut candidate_scores,
            vec![vector],
            "e1234",
            SEMANTIC_WEIGHT,
            RetrievalLeg::Vector,
        );
        let ranked = rank_candidates(candidate_scores.into_values().collect(), 5);
        let paths: Vec<&str> = ranked.iter().map(|c| c.doc.path.as_str()).collect();
        assert!(paths.contains(&"notas/lexico.md"), "got {paths:?}");
        assert!(paths.contains(&"notas/vector.md"), "got {paths:?}");
    }

    fn remove_embedding_env_keys() {
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
    }

    /// The short precisely-relevant Spanish doc must outrank a 200-fold
    /// repetition spam doc, and repeated runs must agree exactly.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hybrid_ranks_spanish_doc_first_in_tempdir() {
        let _temp_env = crate::settings::tests::TempEnv::new();
        remove_embedding_env_keys();
        // `tempfile` is NOT a dev-dependency: build the scratch dir by hand.
        let data_dir =
            std::env::temp_dir().join(format!("xavier-hybrid-rank-{}", std::process::id()));
        std::fs::create_dir_all(&data_dir).expect("create scratch data dir");
        std::env::set_var("XAVIER_DATA_DIR", &data_dir);

        let ruido = vec!["factura luz viernes pagar"; 200].join(" ");
        let docs = vec![
            neutral_kind_doc(
                "notas/factura.md",
                "la factura de la luz vence el viernes y hay que pagarla",
            ),
            neutral_kind_doc(
                "notas/gato.md",
                "el gato duerme en el sofá todas las tardes",
            ),
            neutral_kind_doc("notas/ruido.md", &ruido),
        ];
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(docs)));

        let first = search_hybrid_optimized(&memory, "factura de la luz", 2, None)
            .await
            .expect("hybrid search must succeed");
        assert!(
            !first.is_empty(),
            "expected candidates for a matching query"
        );
        assert_eq!(first[0].path, "notas/factura.md");
        assert!(first.len() <= 2);

        let second = search_hybrid_optimized(&memory, "factura de la luz", 2, None)
            .await
            .expect("hybrid search must succeed");
        let first_paths: Vec<&str> = first.iter().map(|doc| doc.path.as_str()).collect();
        let second_paths: Vec<&str> = second.iter().map(|doc| doc.path.as_str()).collect();
        assert_eq!(
            first_paths, second_paths,
            "repeated runs must agree exactly"
        );

        std::fs::remove_dir_all(&data_dir).ok();
    }

    /// The vector leg must use the real cosine similarity, not the rank
    /// constant: a doc with cosine 1.0 but no query term must beat a doc
    /// that only matches lexically (rank-only fusion scored 0.7/61 vs
    /// 0.3/61 and the lexical doc won).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn query_with_hybrid_search_uses_real_vector_similarity() {
        let _temp_env = crate::settings::tests::TempEnv::new();
        remove_embedding_env_keys();
        let query_vector = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        // Neutral kind: the vector doc must NOT match lexically (the `File`
        // kind bonus would make it a keyword hit too and mask the signal
        // under test); the lexical doc keeps matching by term only.
        let mut vector_doc = neutral_kind_doc("notas/vector.md", "el gato duerme en el sofá");
        vector_doc.embedding = query_vector.clone();
        let mut lexical_doc = neutral_kind_doc("notas/lexico.md", "manual del qubit cuántico");
        lexical_doc.embedding = vec![0.0; TEST_DIM];
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(vec![lexical_doc, vector_doc])));

        let results = query_with_hybrid_search(&memory, "qubit", query_vector, 5)
            .await
            .expect("hybrid search must succeed");
        assert!(!results.is_empty(), "expected candidates");
        assert_eq!(results[0].path, "notas/vector.md");
    }

    /// A query matching nothing must return no candidates (no
    /// context-expansion noise promoted to results).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unrelated_spanish_query_returns_no_candidates() {
        let _temp_env = crate::settings::tests::TempEnv::new();
        remove_embedding_env_keys();
        let data_dir = std::env::temp_dir().join(format!(
            "xavier-hybrid-rank-unrelated-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&data_dir).expect("create scratch data dir");
        std::env::set_var("XAVIER_DATA_DIR", &data_dir);

        let ruido = vec!["factura luz viernes pagar"; 200].join(" ");
        let docs = vec![
            neutral_kind_doc(
                "notas/factura.md",
                "la factura de la luz vence el viernes y hay que pagarla",
            ),
            neutral_kind_doc(
                "notas/gato.md",
                "el gato duerme en el sofá todas las tardes",
            ),
            neutral_kind_doc("notas/ruido.md", &ruido),
        ];
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(docs)));

        let results = search_hybrid_optimized(&memory, "fotosíntesis clorofila", 5, None)
            .await
            .expect("hybrid search must succeed");
        assert!(
            results.is_empty(),
            "unrelated query must return no candidates"
        );

        std::fs::remove_dir_all(&data_dir).ok();
    }
}
