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
use super::vector::vsearch;

/// Hybrid search with variant expansion, multi-hop context, and RRF re-ranking.
#[autometrics]
pub async fn search_hybrid_optimized(
    memory: &QmdMemory,
    query_text: &str,
    limit: usize,
    filters: Option<&MemoryQueryFilters>,
) -> Result<Vec<MemoryDocument>> {
    let query_bundle = query_builder::build_query_bundle_internal(query_text);
    let mut candidate_scores: HashMap<String, (f32, MemoryDocument, f32)> = HashMap::new();

    for expanded_query in &query_bundle.variants {
        let cache_hit = memory
            .search_with_cache_filtered(expanded_query, limit.max(3), filters)
            .await?;
        merge_ranked_candidates(
            &mut candidate_scores,
            cache_hit.documents,
            expanded_query,
            query_bundle.weight_for(expanded_query),
        );
    }

    if candidate_scores.is_empty() {
        return Ok(Vec::new());
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

    Ok(reranked
        .into_iter()
        .take(limit)
        .map(|(score, mut doc, _)| {
            doc.score = score;
            doc
        })
        .collect())
}

/// Merge ranked candidates using RRF + contextual boost.
pub fn merge_ranked_candidates(
    candidate_scores: &mut HashMap<String, (f32, MemoryDocument, f32)>,
    documents: Vec<MemoryDocument>,
    query: &str,
    query_weight: f32,
) {
    for (rank, doc) in documents.into_iter().enumerate() {
        let key = doc.id.clone().unwrap_or_else(|| doc.path.clone());
        let rrf_score = 1.0 / (RRF_K + (rank as f32) + 1.0);
        let rerank = contextual_boost(query, &doc, query_weight);
        let combined = (rrf_score * query_weight) + rerank;
        candidate_scores
            .entry(key)
            .and_modify(|entry| {
                entry.0 += combined;
                entry.2 = entry.2.max(query_weight);
            })
            .or_insert((combined, doc, query_weight));
    }
}

/// Adaptive hybrid search combining lexical and vector retrieval with dynamic query-based weights.
pub async fn query_with_adaptive_search(
    memory: &QmdMemory,
    query_text: &str,
    query_vector: Vec<f32>,
    limit: usize,
) -> Result<Vec<MemoryDocument>> {
    let weights = get_query_weights(query_text);

    // Log selected weights for debugging/verification
    tracing::info!(
        "Adaptive hybrid search: query='{}' classified with keyword_weight={} and semantic_weight={}",
        query_text, weights.keyword, weights.semantic
    );

    let mut scores: HashMap<String, (f32, MemoryDocument)> = HashMap::new();

    // Perform lexical search if keyword weight is non-zero
    if weights.keyword > 0.0 {
        let keyword_hits = memory
            .search_with_cache_filtered(query_text, limit, None)
            .await?;
        for (rank, doc) in keyword_hits.documents.into_iter().enumerate() {
            let key = doc
                .id
                .clone()
                .unwrap_or_else(|| format!("path:{}", doc.path));
            let rrf_score = 1.0 / (RRF_K + rank as f32 + 1.0);
            scores.insert(key, (rrf_score * weights.keyword, doc));
        }
    }

    // Perform vector search if semantic weight is non-zero
    if weights.semantic > 0.0 && !query_vector.is_empty() {
        let vector_hits = vsearch(memory, query_vector, limit).await?;
        for (rank, doc) in vector_hits.into_iter().enumerate() {
            let key = doc
                .id
                .clone()
                .unwrap_or_else(|| format!("path:{}", doc.path));
            let rrf_score = 1.0 / (RRF_K + rank as f32 + 1.0);
            if let Some((existing, _)) = scores.get_mut(&key) {
                *existing += rrf_score * weights.semantic;
            } else {
                scores.insert(key, (rrf_score * weights.semantic, doc));
            }
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

/// Dynamic helper to resolve weights using either the external classifier or inline fallback heuristics.
pub fn get_query_weights(query_text: &str) -> QueryClassWeights {
    #[cfg(has_classifier_module)]
    {
        super::classifier::classify_query(query_text)
    }
    #[cfg(not(has_classifier_module))]
    {
        classify_query_inline(query_text)
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
        vsearch(memory, query_vector.clone(), limit)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|doc| {
                crate::memory::schema::matches_filters(
                    &doc.path,
                    &doc.metadata,
                    &memory.workspace_id,
                    filters,
                )
            })
            .collect()
    };

    if vector_results.is_empty() && query_vector.is_empty() {
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


#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QueryClassWeights {
    pub keyword: f32,
    pub semantic: f32,
}

/// Fallback inline query classifier based on heuristics.
pub fn classify_query_inline(query: &str) -> QueryClassWeights {
    let query_trimmed = query.trim();
    if query_trimmed.is_empty() {
        return QueryClassWeights {
            keyword: 0.5,
            semantic: 0.5,
        };
    }

    let mut code_signals = 0;
    let mut semantic_signals = 0;

    // 1. Símbolos de código y patrones de operadores
    if query_trimmed.contains("::") || query_trimmed.contains("->") || query_trimmed.contains("()") {
        code_signals += 3;
    }

    for c in ['{', '}', '[', ']', '<', '>', '=', '+', '*', '!', '&', '|', '_', '/', '\\'] {
        if query_trimmed.contains(c) {
            code_signals += 1;
        }
    }

    // 2. Acrónimos técnicos y términos específicos (por ejemplo FTS5, SQLite, vec)
    let technical_terms = [
        "fts", "fts5", "sqlite", "vec", "api", "cli", "gllm", "mcp", "p2p", "dag", "ttl", "rrf", "db", "sql", "json"
    ];
    let words: Vec<&str> = query_trimmed.split_whitespace().collect();
    for word in &words {
        let clean_word = word.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        if technical_terms.contains(&clean_word.as_str()) {
            code_signals += 2;
        }

        // Acrónimos completamente en mayúsculas (por ejemplo, FTS5 o SQL)
        if word.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) && word.len() >= 3 {
            code_signals += 1;
        }

        // Caso mixto no-inicial (por ejemplo SQLite, SQLiteVec, WebAssembly)
        let has_upper = word.chars().any(|c| c.is_ascii_uppercase());
        let has_lower = word.chars().any(|c| c.is_ascii_lowercase());
        if has_upper && has_lower && !word.starts_with(|c: char| c.is_ascii_uppercase()) {
            code_signals += 1;
        }
    }

    // 3. Términos conceptuales (general/semantic search)
    let conceptual_terms = [
        "arquitectura", "memoria", "agente", "agentes", "architecture", "memory", "agent", "agents",
        "concept", "theory", "how", "why", "explain", "design", "general", "abstract", "overview"
    ];
    for word in &words {
        let clean_word = word.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        if conceptual_terms.contains(&clean_word.as_str()) {
            semantic_signals += 2;
        }
    }

    // Retorna pesos basados en las señales detectadas
    if code_signals > semantic_signals {
        // BM25-weighted mode: coincide con coincidencia exacta dominante
        QueryClassWeights {
            keyword: 0.8,
            semantic: 0.2,
        }
    } else if semantic_signals > code_signals {
        // Vector-weighted mode: coincide con similitud semántica dominante
        QueryClassWeights {
            keyword: 0.2,
            semantic: 0.8,
        }
    } else {
        // Balanced hybrid mode
        QueryClassWeights {
            keyword: 0.5,
            semantic: 0.5,
        }
    }
}
