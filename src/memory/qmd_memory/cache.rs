use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Instant;
use tokio::sync::RwLock as AsyncRwLock;

use crate::memory::schema::matches_filters;
use crate::memory::qmd_memory::search::lexical_score;
use crate::memory::qmd_memory::types::{CachedSearchResult, EmbeddingCacheEntry, MemoryDocument, SearchCacheKey};
use crate::memory::qmd_memory::utils::normalize_query;
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::schema::MemoryQueryFilters;
use crate::utils::crypto::hex_encode;
use std::sync::atomic::Ordering as AtomicOrdering;

pub const EMBEDDING_CACHE_TTL_SECS: u64 = 3600; // 1 hour

pub static EMBEDDING_CACHE: LazyLock<Arc<AsyncRwLock<HashMap<String, EmbeddingCacheEntry>>>> =
    LazyLock::new(|| Arc::new(AsyncRwLock::new(HashMap::new())));

pub fn embedding_cache_key(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex_encode(&hasher.finalize())
}

pub async fn clean_embedding_cache() {
    let mut cache = EMBEDDING_CACHE.write().await;
    let now = Instant::now();
    cache.retain(|_, entry| {
        now.duration_since(entry.cached_at).as_secs() < EMBEDDING_CACHE_TTL_SECS
    });
}

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

    let embedder =
        crate::adapters::outbound::embedding::embedding_adapter::build_embedding_port_from_env()?;
    for attempt in 0..3 {
        match embedder.embed(&preprocessed).await {
            Ok(vector) => {
                let mut cache = EMBEDDING_CACHE.write().await;
                cache.insert(
                    cache_key,
                    EmbeddingCacheEntry {
                        vector: vector.clone(),
                        cached_at: Instant::now(),
                    },
                );
                if cache.len() % 10 == 0 {
                    drop(cache);
                    clean_embedding_cache().await;
                }
                return Ok(vector);
            }
            Err(error) => {
                last_error = Some(error);
                if attempt < 2 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms * 2).min(max_delay_ms);
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("embedding generation failed")))
}

pub fn preprocess_for_embedding(text: &str) -> String {
    let speakers = crate::memory::qmd_memory::utils::extract_speakers(text);

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

pub fn preserve_quoted_speech(text: &str) -> String {
    // Replace quoted text with a marker to emphasize it in embeddings
    let mut result = text.to_string();

    // Pattern for quoted speech: "..." or '...'
    let quote_re = regex::Regex::new(r#"["']([^"']+)["']"#).expect("test assertion");

    let mut quote_count = 0;
    result = quote_re
        .replace_all(&result, |caps: &regex::Captures| {
            quote_count += 1;
            let quote = &caps[1];
            format!("[QUOTE{}: {}]", quote_count, quote)
        })
        .to_string();

    result
}

pub async fn search_with_cache_filtered(
    memory: &QmdMemory,
    query_text: &str,
    limit: usize,
    filters: Option<&MemoryQueryFilters>,
) -> Result<CachedSearchResult> {
    let normalized_query = normalize_query(query_text);
    let cache_key = SearchCacheKey {
        workspace_id: memory.workspace_id.clone(),
        query: normalized_query.clone(),
        limit,
        filters: serde_json::to_string(&filters).unwrap_or_default(),
    };

    if let Some(cached) = memory.search_cache.read().await.get(&cache_key).cloned() {
        memory.cache_counters
            .hits
            .fetch_add(1, AtomicOrdering::Relaxed);
        return Ok(CachedSearchResult {
            documents: cached,
            cache_hit: true,
        });
    }

    let docs = memory.docs.read().await;
    let mut scored: Vec<(f32, MemoryDocument)> = docs
        .iter()
        .filter_map(|doc| {
            if !matches_filters(&doc.path, &doc.metadata, &memory.workspace_id, filters) {
                return None;
            }
            let score = lexical_score(doc, &normalized_query);
            (score > 0.0).then(|| (score, doc.clone()))
        })
        .collect();
    drop(docs);

    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.path.cmp(&right.1.path))
    });

    let documents: Vec<MemoryDocument> =
        scored.into_iter().map(|(_, doc)| doc).take(limit).collect();

    memory.search_cache
        .write()
        .await
        .insert(cache_key, documents.clone());
    memory.cache_counters
        .misses
        .fetch_add(1, AtomicOrdering::Relaxed);

    Ok(CachedSearchResult {
        documents,
        cache_hit: false,
    })
}

pub async fn invalidate_cache(memory: &QmdMemory) {
    memory.search_cache.write().await.clear();
}
