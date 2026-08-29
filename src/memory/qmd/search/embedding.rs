//! Embedding-based search with pronoun resolution and query expansion.
//!
//! Generates query embeddings, resolves pronouns against known speakers,
//! expands queries with context terms, and performs filtered retrieval.

use anyhow::Result;

use crate::memory::qmd_memory::reader::generate_embedding;
use crate::memory::qmd_memory::types::MemoryDocument;
use crate::memory::qmd_memory::utils::*;
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::schema::MemoryQueryFilters;

use super::hybrid::query_filtered;
use super::vector::vsearch;

/// Embedding-based search without filters.
pub async fn query_with_embedding(
    memory: &QmdMemory,
    query_text: &str,
    limit: usize,
) -> Result<Vec<MemoryDocument>> {
    query_with_embedding_filtered(memory, query_text, limit, None)
        .await
        .map(|r| r.documents)
}

/// Result of an embedding-based search, including degradation status.
pub struct EmbeddingSearchResult {
    pub documents: Vec<MemoryDocument>,
    pub degraded: bool,
}

/// Embedding-based search with optional filters.
pub async fn query_with_embedding_filtered(
    memory: &QmdMemory,
    query_text: &str,
    limit: usize,
    filters: Option<&MemoryQueryFilters>,
) -> Result<EmbeddingSearchResult> {
    let mut processed_query = query_text.to_string();

    let all_docs = memory.all_documents().await;
    let mut all_speakers = std::collections::HashSet::new();
    let locomo_only = !all_docs.is_empty()
        && all_docs
            .iter()
            .all(|doc| is_locomo_document(&doc.path, &doc.metadata));
    for doc in &all_docs {
        for speaker in extract_speakers(&doc.content) {
            all_speakers.insert(speaker);
        }
    }
    let speakers_list: Vec<String> = all_speakers.into_iter().collect();

    if !speakers_list.is_empty() {
        processed_query = resolve_pronouns(&processed_query, &speakers_list);
    }

    if let Some(target_speaker) = extract_speaker_from_query(query_text) {
        if !processed_query.contains(&target_speaker) {
            processed_query = format!("{} {}", target_speaker, processed_query);
        }
    }

    if locomo_only {
        return query_filtered(memory, &processed_query, Vec::new(), limit, filters)
            .await
            .map(|docs| EmbeddingSearchResult {
                documents: docs,
                degraded: false,
            });
    }

    let timeout_ms = std::env::var("XAVIER_EMBEDDING_FALLBACK_BUDGET_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(2000);

    let (query_vector, degraded) = match tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        generate_embedding(&processed_query),
    )
    .await
    {
        Ok(Ok(v)) if !v.is_empty() => (v, false),
        Ok(Ok(_)) => (Vec::new(), true),
        Ok(Err(e)) => {
            tracing::warn!(
                error = %e,
                "embedding generation failed, falling back to BM25/substring"
            );
            (Vec::new(), true)
        }
        Err(_) => {
            tracing::warn!(
                timeout_ms = timeout_ms,
                "embedding generation timed out, falling back to BM25/substring"
            );
            (Vec::new(), true)
        }
    };

    if query_vector.is_empty() {
        return memory
            .search_with_cache_filtered(&processed_query, limit, filters)
            .await
            .map(|r| EmbeddingSearchResult {
                documents: r.documents,
                degraded,
            });
    }

    let initial_results = vsearch(memory, query_vector.clone(), 3)
        .await
        .unwrap_or_default();

    if !initial_results.is_empty() {
        let mut context_terms = Vec::new();

        let common_words: std::collections::HashSet<&str> = std::collections::HashSet::from_iter([
            "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has",
            "had", "do", "does", "did", "will", "would", "could", "should", "may", "might", "must",
            "shall", "can", "need", "dare", "to", "of", "in", "for", "on", "with", "at", "by",
            "from", "as", "into", "through", "during", "before", "after", "above", "below", "that",
            "this", "these", "those", "it", "its", "they", "them", "what", "which", "who", "whom",
            "whose", "where", "when", "why", "how",
        ]);

        for doc in initial_results.iter().take(2) {
            for word in doc.content.split_whitespace() {
                let w_clean = word
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase();
                if w_clean.len() >= 4
                    && !common_words.contains(w_clean.as_str())
                    && !processed_query.to_lowercase().contains(&w_clean)
                {
                    context_terms.push(w_clean);
                }
            }
        }

        if context_terms.len() >= 2 {
            let expanded_query = format!("{} {}", processed_query, context_terms.join(" "));
            if let Ok(expanded_vector) = generate_embedding(&expanded_query).await {
                if !expanded_vector.is_empty() {
                    return query_filtered(
                        memory,
                        &expanded_query,
                        expanded_vector,
                        limit,
                        filters,
                    )
                    .await
                    .map(|docs| EmbeddingSearchResult {
                        documents: docs,
                        degraded: false,
                    });
                }
            }
        }
    }

    query_filtered(memory, &processed_query, query_vector, limit, filters)
        .await
        .map(|docs| EmbeddingSearchResult {
            documents: docs,
            degraded,
        })
}
