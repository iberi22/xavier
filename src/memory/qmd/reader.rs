//! QMD document reader
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering as AtomicOrdering;
use std::time::Instant;

use crate::memory::qmd_memory::config::EMBEDDING_CACHE;
use crate::memory::qmd_memory::config::EMBEDDING_CACHE_TTL_SECS;
use crate::memory::qmd_memory::query_builder::normalize_query;
use crate::memory::qmd_memory::search::scoring::LexicalBatchScorer;
use crate::memory::qmd_memory::types::{
    CachedSearchResult, EmbeddingCacheEntry, MemoryDocument, MemoryUsage, SearchCacheKey,
};
use crate::memory::qmd_memory::utils::extract_speakers;
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::schema::matches_filters;
use crate::memory::schema::MemoryQueryFilters;
use crate::utils::crypto::hex_encode;

// ── Embedding cache operations ───────────────────────────────────────

/// Embedding cache key.
pub fn embedding_cache_key(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex_encode(&hasher.finalize())
}

/// Clean embedding cache.
pub async fn clean_embedding_cache() {
    let mut cache = EMBEDDING_CACHE.write().await;
    let now = Instant::now();
    cache.retain(|_, entry| {
        now.duration_since(entry.cached_at).as_secs() < EMBEDDING_CACHE_TTL_SECS
    });
}

/// Generate embedding.
pub async fn generate_embedding(text: &str) -> Result<Vec<f32>> {
    if !crate::memory::embedder::EmbeddingClient::is_configured_from_env() {
        return Ok(Vec::new());
    }
    let preprocessed = preprocess_for_embedding(text);
    let cache_key = embedding_cache_key(&preprocessed);

    // CRITICAL FIX: Check embedding cache first to avoid redundant API calls
    {
        let cache = EMBEDDING_CACHE.read().await;
        if let Some(entry) = cache.get(&cache_key) {
            // Check if entry is still valid (within TTL)
            if Instant::now().duration_since(entry.cached_at).as_secs() < EMBEDDING_CACHE_TTL_SECS {
                tracing::debug!("Embedding cache HIT for key: {}", &cache_key[..16]);
                return Ok(entry.vector.clone());
            }
        }
    }

    let mut last_error = None;
    let mut delay_ms: u64 = 100;
    let max_delay_ms: u64 = 2000;
    let embed_timeout = crate::memory::embedder::embedding_timeout();

    let embedder = crate::memory::embedder::EmbeddingClient::from_env()?;
    for attempt in 0..3 {
        let embed_future = embedder.embed(&preprocessed);
        match tokio::time::timeout(embed_timeout, embed_future).await {
            Ok(Ok(vector)) => {
                let mut cache = EMBEDDING_CACHE.write().await;
                cache.insert(
                    cache_key,
                    EmbeddingCacheEntry {
                        vector: vector.clone(),
                        cached_at: Instant::now(),
                    },
                );
                // Bound memory: global HashMap had no capacity limit (only
                // 1h TTL, cleaned every 10 inserts). Under bulk reindex it
                // grew without bound (each 768d vec ~3KB + overhead).
                // Cap at 5000 entries; evict expired first, then arbitrary
                // oldest batch if still over budget.
                const MAX_EMBEDDING_CACHE_ENTRIES: usize = 5000;
                if cache.len() > MAX_EMBEDDING_CACHE_ENTRIES {
                    let now = Instant::now();
                    cache.retain(|_, entry| {
                        now.duration_since(entry.cached_at).as_secs() < EMBEDDING_CACHE_TTL_SECS
                    });
                }
                if cache.len() > MAX_EMBEDDING_CACHE_ENTRIES {
                    let excess = cache.len() - MAX_EMBEDDING_CACHE_ENTRIES;
                    let keys: Vec<String> = cache.keys().take(excess).cloned().collect();
                    for key in keys {
                        cache.remove(&key);
                    }
                }
                let needs_clean = cache.len() % 10 == 0;
                drop(cache);
                if needs_clean {
                    clean_embedding_cache().await;
                }
                return Ok(vector);
            }
            Ok(Err(error)) => {
                last_error = Some(error);
                if attempt < 2 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms * 2).min(max_delay_ms);
                }
            }
            Err(_timeout_err) => {
                last_error = Some(anyhow::anyhow!(
                    "embedding request timed out after {:?}",
                    embed_timeout
                ));
                if attempt < 2 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms * 2).min(max_delay_ms);
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("embedding generation failed")))
}

/// Preprocess for embedding.
pub fn preprocess_for_embedding(text: &str) -> String {
    let speakers = extract_speakers(text);

    if speakers.is_empty() {
        // Still preprocess to handle quoted speech
        return preserve_quoted_speech(text);
    }

    // Build structured speaker context with turn-taking info
    let speaker_list: Vec<String> = speakers.iter().map(|s| format!("[{}]", s)).collect();

    // Preserve quoted speech which often contains answers
    let text_with_quotes = preserve_quoted_speech(text);

    let speaker_ctx = format!(
        "Conversation between: {}. \nQuote context: {}\n\n",
        speaker_list.join(", "),
        speaker_list.join(" said, ")
    );
    format!("{}{}", speaker_ctx, text_with_quotes)
}

/// Preserve quoted speech.
pub fn preserve_quoted_speech(text: &str) -> String {
    // Static regex: compiling on every embedding/search call burned CPU
    // (Regex::new per call). Hoist to a LazyLock so the hot path reuses it.
    static QUOTE_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"["']([^"']+)["']"#).expect("valid quote regex")
    });
    // Replace quoted text with a marker to emphasize it in embeddings
    let mut result = text.to_string();
    let mut quote_count = 0;
    result = QUOTE_RE
        .replace_all(&result, |caps: &regex::Captures| {
            quote_count += 1;
            let quote = &caps[1];
            format!("[QUOTE{}: {}]", quote_count, quote)
        })
        .to_string();

    result
}

// ── Search cache operations ───────────────────────────────────────────

/// Search with cache filtered.
pub async fn search_with_cache_filtered(
    memory: &QmdMemory,
    query_text: &str,
    limit: usize,
    filters: Option<&MemoryQueryFilters>,
) -> Result<CachedSearchResult> {
    Ok(
        search_queries_with_cache_filtered(memory, &[query_text.to_string()], limit, filters, None)
            .await?
            .remove(0)
            .expect("direct lexical retrieval always completes"),
    )
}

/// Rank all uncached variants in one corpus pass, retaining every fusion vote.
pub(crate) async fn search_queries_with_cache_filtered(
    memory: &QmdMemory,
    queries: &[String],
    limit: usize,
    filters: Option<&MemoryQueryFilters>,
    optional_deadline: Option<tokio::time::Instant>,
) -> Result<Vec<Option<CachedSearchResult>>> {
    let keys: Vec<_> = queries
        .iter()
        .map(|query| SearchCacheKey {
            workspace_id: memory.workspace_id.clone(),
            query: normalize_query(query),
            limit,
            filters: serde_json::to_string(&filters).unwrap_or_default(),
        })
        .collect();
    let mut results = Vec::with_capacity(keys.len());
    let mut missing = Vec::new();
    {
        let cache = memory.search_cache.read().await;
        for (index, key) in keys.iter().enumerate() {
            if let Some(documents) = cache.get(key) {
                memory
                    .cache_counters
                    .hits
                    .fetch_add(1, AtomicOrdering::Relaxed);
                results.push(Some(CachedSearchResult {
                    documents: documents.clone(),
                    cache_hit: true,
                }));
            } else {
                missing.push(index);
                results.push(None);
            }
        }
    }
    if missing.is_empty() {
        return Ok(results);
    }
    let include_activity = filters.and_then(|f| f.include_activity).unwrap_or(false);
    let docs = memory.docs.clone();
    let workspace_id = memory.workspace_id.clone();
    let owned_filters = filters.cloned();
    let scoring_queries = missing
        .iter()
        .map(|index| keys[*index].query.clone())
        .collect();
    let cancelled = tokio_util::sync::CancellationToken::new();
    let _cancel_on_drop = cancelled.clone().drop_guard();
    let retrieval = tokio::task::spawn_blocking(move || {
        if cancelled.is_cancelled() {
            return Vec::new();
        }
        let docs = docs.blocking_read();
        if cancelled.is_cancelled() {
            return Vec::new();
        }
        let scorer = LexicalBatchScorer::new(scoring_queries);
        let mut rankings: Vec<Vec<(f32, usize)>> = Vec::new();
        for (index, doc) in docs
            .iter()
            .enumerate()
            .take_while(|_| !cancelled.is_cancelled())
        {
            if !matches_filters(
                &doc.path,
                &doc.metadata,
                &workspace_id,
                owned_filters.as_ref(),
            ) || (!include_activity && crate::memory::schema::is_noise_content(&doc.content))
            {
                continue;
            }
            let scores = scorer.scores(doc);
            rankings.resize_with(scores.len(), Vec::new);
            for (ranking, score) in rankings.iter_mut().zip(scores) {
                if score > 0.0 {
                    ranking.push((score, index));
                }
            }
        }
        rankings
            .into_iter()
            .map(|mut scored| {
                let compare = |left: &(f32, usize), right: &(f32, usize)| {
                    right
                        .0
                        .partial_cmp(&left.0)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| docs[left.1].path.cmp(&docs[right.1].path))
                        .then_with(|| left.1.cmp(&right.1))
                };
                if scored.len() > limit {
                    scored.select_nth_unstable_by(limit, compare);
                    scored.truncate(limit);
                }
                scored.sort_unstable_by(compare);
                scored
                    .into_iter()
                    .map(|(_, index)| docs[index].clone())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    });
    // A cached direct result must survive optional variants waiting for a writer.
    let documents = if results.first().is_some_and(Option::is_some) {
        if let Some(deadline) = optional_deadline {
            match tokio::time::timeout_at(deadline, retrieval).await {
                Ok(result) => result?,
                Err(_) => return Ok(results),
            }
        } else {
            retrieval.await?
        }
    } else {
        retrieval.await?
    };
    let mut cache = memory.search_cache.write().await;
    for (position, index) in missing.into_iter().enumerate() {
        let documents = documents.get(position).cloned().unwrap_or_default();
        if cache.len() >= 1000 {
            cache.clear();
        }
        cache.insert(keys[index].clone(), documents.clone());
        memory
            .cache_counters
            .misses
            .fetch_add(1, AtomicOrdering::Relaxed);
        results[index] = Some(CachedSearchResult {
            documents,
            cache_hit: false,
        });
    }
    Ok(results)
}

/// Invalidate cache.
pub async fn invalidate_cache(memory: &QmdMemory) {
    memory.search_cache.write().await.clear();
    crate::search::hybrid::invalidate_hybrid_cache().await;
}

// ── Read / query operations ───────────────────────────────────────────

/// Init.
pub async fn init(memory: &QmdMemory) -> Result<()> {
    if let Some(store) = memory.store().await {
        let state = store.load_workspace_state(&memory.workspace_id).await?;
        let docs: Vec<MemoryDocument> = state
            .memories
            .into_iter()
            .map(|record| record.to_document())
            .collect();
        let loaded_memories = docs.len();
        *memory.docs.write().await = docs;
        tracing::info!(
            workspace_id = %memory.workspace_id,
            loaded_memories = loaded_memories,
            "QmdMemory loaded from persistent store"
        );
    }
    Ok(())
}

/// Get.
pub async fn get(memory: &QmdMemory, path_or_id: &str) -> Result<Option<MemoryDocument>> {
    {
        let docs = memory.docs.read().await;
        if let Some(doc) = docs
            .iter()
            .find(|doc| doc.path == path_or_id || doc.id.as_deref() == Some(path_or_id))
        {
            return Ok(Some(doc.clone()));
        }
    }

    if let Some(store) = memory.store().await {
        if let Ok(Some(record)) = store.get(&memory.workspace_id, path_or_id).await {
            return Ok(Some(record.to_document()));
        }
    }

    Ok(None)
}

/// Usage.
pub async fn usage(memory: &QmdMemory) -> MemoryUsage {
    let docs = memory.docs.read().await;
    MemoryUsage {
        document_count: docs.len(),
        storage_bytes: docs.iter().map(MemoryDocument::estimated_bytes).sum(),
    }
}

/// Cache metrics.
pub async fn cache_metrics(memory: &QmdMemory) -> crate::memory::qmd_memory::types::CacheMetrics {
    crate::memory::qmd_memory::types::CacheMetrics {
        hits: memory.cache_counters.hits.load(AtomicOrdering::Relaxed),
        misses: memory.cache_counters.misses.load(AtomicOrdering::Relaxed),
        entries: memory.search_cache.read().await.len(),
    }
}

/// Export.
pub async fn export(memory: &QmdMemory, public_only: bool) -> Result<Vec<MemoryDocument>> {
    let docs = memory.docs.read().await;
    let mut exported: Vec<MemoryDocument> = docs
        .iter()
        .filter(|doc| {
            if !public_only {
                return true;
            }
            // If public_only, check metadata for visibility
            let is_private = doc
                .metadata
                .get("is_private")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let visibility = doc
                .metadata
                .get("visibility")
                .and_then(|v| v.as_str())
                .unwrap_or("public");

            !is_private && visibility != "private"
        })
        .cloned()
        .collect();

    // Auto-redact sensitive PII data before exporting/sharing
    let redaction_engine = crate::security::redaction::RedactionEngine::default();
    for doc in &mut exported {
        doc.content = redaction_engine.redact(&doc.content);
    }

    Ok(exported)
}

#[cfg(test)]
mod latency_regression {
    use super::*;
    use std::future::Future;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn batch_rankings_match_scalar_search_with_filters_and_partial_cache() {
        use crate::memory::qmd_memory::search::lexical_score;
        let docs: Vec<_> = [
            ("a", "bbb bbb bbb bbb aaa", "left"),
            ("b", "aaa aaa aaa aaa bbb", "left"),
            ("c", "bbb bbb aaa bbb", "left"),
            ("d", "aaaaa ééé", "left"),
            ("e", "aaa bbb aaa bbb aaa", "right"),
        ]
        .into_iter()
        .map(|(id, content, namespace)| MemoryDocument {
            id: Some(id.into()),
            path: format!("docs/{id}"),
            content: content.into(),
            metadata: serde_json::json!({"kind": "document", "workspace_id": namespace}),
            ..Default::default()
        })
        .collect();
        let memory = QmdMemory::new_with_workspace(Arc::new(RwLock::new(docs.clone())), "left");
        let queries = [
            "aaa bbb",
            "aaa bbb aaa",
            "aaa bbb bbb",
            "aa aaa aa",
            "é éé",
            "unmatched",
        ]
        .map(str::to_string)
        .to_vec();
        memory
            .search_with_cache_filtered(&queries[0], 3, None)
            .await
            .unwrap();
        let filter = MemoryQueryFilters {
            workspace_id: Some("left".into()),
            include_activity: Some(true),
            ..Default::default()
        };
        for filters in [None, Some(&filter)] {
            let results = search_queries_with_cache_filtered(&memory, &queries, 3, filters, None)
                .await
                .unwrap();
            for (query, actual) in queries.iter().zip(results) {
                let mut expected: Vec<_> = docs
                    .iter()
                    .filter(|doc| matches_filters(&doc.path, &doc.metadata, "left", filters))
                    .map(|doc| (lexical_score(doc, query), doc.path.clone()))
                    .filter(|(score, _)| *score > 0.0)
                    .collect();
                expected.sort_by(|left, right| {
                    right
                        .0
                        .partial_cmp(&left.0)
                        .unwrap()
                        .then_with(|| left.1.cmp(&right.1))
                });
                expected.truncate(3);
                assert_eq!(
                    actual
                        .unwrap()
                        .documents
                        .iter()
                        .map(|doc| doc.path.clone())
                        .collect::<Vec<_>>(),
                    expected
                        .into_iter()
                        .map(|(_, path)| path)
                        .collect::<Vec<_>>(),
                    "query={query}"
                );
            }
        }
    }

    #[test]
    fn cancelled_queued_scans_release_document_lock() {
        use std::time::Duration;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            for vector in [false, true] {
                let memory = QmdMemory::new(Arc::new(RwLock::new(vec![MemoryDocument::default()])));
                let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
                let (release_tx, release_rx) = std::sync::mpsc::channel();
                let blocker = tokio::task::spawn_blocking(move || {
                    ready_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                });
                ready_rx.await.unwrap();
                let search: std::pin::Pin<Box<dyn Future<Output = Result<()>>>> = if vector {
                    Box::pin(async {
                        crate::memory::qmd_memory::search::vector::vsearch_filtered(
                            &memory,
                            vec![1.0],
                            1,
                            None,
                        )
                        .await
                        .map(|_| ())
                    })
                } else {
                    Box::pin(async {
                        search_with_cache_filtered(&memory, "query", 1, None)
                            .await
                            .map(|_| ())
                    })
                };
                let mut search = search;
                assert!(
                    std::future::poll_fn(|cx| std::task::Poll::Ready(
                        search.as_mut().poll(cx).is_pending()
                    ))
                    .await
                );
                drop(search);
                let write =
                    tokio::time::timeout(Duration::from_millis(200), memory.docs.write()).await;
                release_tx.send(()).unwrap();
                assert!(
                    write.is_ok(),
                    "cancelled queued scan retained the read guard (vector={vector})"
                );
                drop(write);
                blocker.await.unwrap();
                // The cancelled closure must skip even acquiring a read guard when dequeued.
                let writer = memory.docs.write().await;
                tokio::time::timeout(
                    Duration::from_millis(200),
                    tokio::task::spawn_blocking(|| ()),
                )
                .await
                .unwrap()
                .unwrap();
                drop(writer);
            }
        });
    }

    #[tokio::test]
    async fn lexical_cache_miss_yields_and_preserves_ranking() {
        let docs = (0..2000)
            .map(|i| MemoryDocument {
                id: Some(i.to_string()),
                path: format!("docs/{i:04}"),
                content: "runtime memoria rust ".repeat(100),
                embedding: vec![0.5; 768],
                ..Default::default()
            })
            .collect();
        let memory = QmdMemory::new(Arc::new(RwLock::new(docs)));
        let started = Instant::now();
        let search =
            memory.search_with_cache_filtered("como configurar runtime memoria en rust", 5, None);
        tokio::pin!(search);
        let yielded = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(search.as_mut().poll(cx).is_pending())
        })
        .await;
        eprintln!("lexical first poll: {:?}", started.elapsed());
        assert!(
            yielded,
            "lexical scoring blocked the Tokio worker until completion"
        );
        let result = search.await.unwrap();
        eprintln!("lexical miss: {:?}", started.elapsed());
        assert!(!result.cache_hit);
        assert_eq!(result.documents.len(), 5);
        assert_eq!(result.documents[0].path, "docs/0000");
        let cached = memory
            .search_with_cache_filtered("como configurar runtime memoria en rust", 5, None)
            .await
            .unwrap();
        assert!(cached.cache_hit);
        assert_eq!(cached.documents[0].path, "docs/0000");
    }
}
