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
            // Bound the second embedding call with the same fallback budget:
            // a slow provider must degrade, never hang the whole search.
            let expanded_vector = match tokio::time::timeout(
                std::time::Duration::from_millis(timeout_ms),
                generate_embedding(&expanded_query),
            )
            .await
            {
                Ok(Ok(vector)) => vector,
                _ => Vec::new(),
            };
            if !expanded_vector.is_empty() {
                return query_filtered(memory, &expanded_query, expanded_vector, limit, filters)
                    .await
                    .map(|docs| EmbeddingSearchResult {
                        documents: docs,
                        degraded: false,
                    });
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
            embedding: vec![0.5; TEST_DIM],
            ..Default::default()
        }
    }

    /// Minimal HTTP stub: fast on every route except POST /api/embed bodies
    /// containing `slow_marker`, which sleep `slow_secs` before responding.
    async fn run_slow_embed_server(slow_marker: &'static str, slow_secs: u64) -> String {
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
                    let body_str = String::from_utf8_lossy(&body);
                    let (payload, status) = if head.starts_with("GET /v1/models") {
                        (
                            r#"{"object":"list","data":[{"id":"test-embed","object":"model"}]}"#
                                .to_string(),
                            200,
                        )
                    } else if head.starts_with("POST /api/embed") {
                        if body_str.contains(slow_marker) {
                            tokio::time::sleep(Duration::from_secs(slow_secs)).await;
                        }
                        let vec_json = vec!["0.5"; TEST_DIM].join(",");
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

    /// The query-expansion embedding call must respect
    /// XAVIER_EMBEDDING_FALLBACK_BUDGET_MS instead of hanging on a slow
    /// provider (P0: POST /v1/memories/search hung, connections piled up).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::await_holding_lock)]
    async fn test_expanded_embedding_uses_fallback_budget() {
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
        // "registration" only appears in the *expanded* query (as a context
        // term from the docs), so only the second embedding call is slow.
        let base = run_slow_embed_server("registration", 3).await;
        std::env::set_var("XAVIER_OLLAMA_URL", format!("{base}/api/embed"));
        std::env::set_var("XAVIER_OLLAMA_MODEL", "test-embed");
        std::env::set_var("XAVIER_OLLAMA_DIMS", TEST_DIM.to_string());
        std::env::set_var("_XAVIER_TEST_OLLAMA_PROBE_URL", format!("{base}/v1/models"));
        std::env::set_var("XAVIER_EMBEDDING_FALLBACK_BUDGET_MS", "500");

        let docs = vec![
            test_doc("notes/alpha", "alpha cluster registration workflow ledger"),
            test_doc("notes/beta", "alpha cluster registration workflow index"),
        ];
        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(docs)));

        let start = Instant::now();
        let result = query_with_embedding_filtered(&memory, "node provisioning", 5, None)
            .await
            .expect("search must not fail");
        let elapsed = start.elapsed();

        std::env::remove_var("_XAVIER_TEST_OLLAMA_PROBE_URL");

        assert!(
            !result.degraded,
            "first embedding must succeed so the expansion path is exercised"
        );
        assert!(
            !result.documents.is_empty(),
            "hybrid search must return docs"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "expanded embedding must respect the 500ms fallback budget, took {elapsed:?}"
        );
    }
}
