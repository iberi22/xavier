use anyhow::Result;
use axum::{routing::post, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::net::TcpListener;
use xavier::embedding::{build_embedder_from_env, Embedder, EmbeddingError, NoopEmbedder};
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::{HybridSearchMode, MemoryRecord, MemoryStore};

async fn start_mock_ollama(dimension: usize) -> Result<(String, tokio::task::JoinHandle<()>)> {
    let app = Router::new().route(
        "/v1/embeddings",
        post(move |Json(payload): Json<Value>| async move {
            let input = payload.get("input").and_then(|i| i.as_str()).unwrap_or("");

            // Generate a deterministic fake embedding
            let mut embedding = vec![0.0f32; dimension];
            for (i, b) in input.as_bytes().iter().enumerate() {
                if i < dimension {
                    embedding[i] = (*b as f32) / 255.0;
                }
            }

            Json(json!({
                "object": "list",
                "data": [
                    {
                        "object": "embedding",
                        "index": 0,
                        "embedding": embedding
                    }
                ],
                "model": "embeddinggemma",
                "usage": {
                    "prompt_tokens": 0,
                    "total_tokens": 0
                }
            }))
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let url = format!("http://{}", addr);

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((url, handle))
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

async fn is_ollama_reachable(url: &str) -> bool {
    let client = reqwest::Client::new();
    match client
        .get(url.replace("/v1/embeddings", "/api/tags"))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

#[tokio::test]
async fn test_encode_dimension_and_similarity() -> Result<()> {
    let test_real = std::env::var("XAVIER_TEST_OLLAMA").unwrap_or_default() == "1";
    let dimension = 768; // Default for embeddinggemma

    let (url, _handle) = if test_real {
        let real_url = "http://localhost:11434/v1/embeddings".to_string();
        if !is_ollama_reachable(&real_url).await {
            println!("Skipping real Ollama test: local instance not reachable");
            return Ok(());
        }
        (real_url, None)
    } else {
        let (mock_url, handle) = start_mock_ollama(dimension).await?;
        (mock_url, Some(handle))
    };

    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "local");
    std::env::set_var("XAVIER_EMBEDDING_LOCAL_URL", &url);
    std::env::set_var("XAVIER_EMBEDDING_MODEL", "embeddinggemma");

    let embedder = build_embedder_from_env().await?;
    assert_eq!(embedder.dimension(), dimension);

    let v1 = embedder.encode("hola").await?;
    assert_eq!(v1.len(), dimension);
    assert!(!v1.iter().all(|&x| x == 0.0));

    let v2 = embedder.encode("hello").await?;
    let v3 = embedder.encode("computadora").await?;

    let sim_same = cosine_similarity(&v1, &v2);
    let sim_diff = cosine_similarity(&v1, &v3);

    println!("Similarity (hola, hello): {}", sim_same);
    println!("Similarity (hola, computadora): {}", sim_diff);

    // Sanity check: same language/similar meaning should be more similar than unrelated words
    // Note: with mock data this might be weird but for real Ollama it should hold.
    if test_real {
        assert!(sim_same > sim_diff);
    }

    Ok(())
}

#[tokio::test]
async fn test_fallback_embedder() -> Result<()> {
    // 1. Mock a failing "cloud" server (Primary)
    let failing_listener = TcpListener::bind("127.0.0.1:0").await?;
    let failing_addr = failing_listener.local_addr()?;
    let failing_url = format!("http://{}", failing_addr);

    let failing_app = Router::new().route(
        "/v1/embeddings",
        post(|| async {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Primary failed",
            )
        }),
    );
    tokio::spawn(async move {
        let _ = axum::serve(failing_listener, failing_app).await;
    });

    // 2. Mock a working "local" server (Fallback)
    let dimension = 384;
    let (success_url, _success_handle) = start_mock_ollama(dimension).await?;

    // 3. Configure Xavier to use "cloud" mode, which in src/embedding/mod.rs
    // creates a Fallback([Cloud, GLLM, Local]) chain if signals are present.
    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "cloud");
    std::env::set_var("XAVIER_EMBEDDING_URL", &failing_url); // Primary
    std::env::set_var("OPENAI_API_KEY", "test-key");

    // Set local signal to trigger inclusion of local backend in the fallback chain
    std::env::set_var("XAVIER_EMBEDDING_LOCAL_URL", &success_url); // Fallback
    std::env::set_var("XAVIER_EMBEDDING_MODEL", "all-minilm"); // 384 dim

    let embedder = build_embedder_from_env().await?;

    // 4. Attempt to encode. It should try failing_url, fail, skip GLLM (if unavailable),
    // and finally succeed with success_url.
    let result = embedder.encode("test fallback").await;

    match result {
        Ok(v) => {
            println!("Fallback succeeded, got vector of size {}", v.len());
            assert_eq!(v.len(), dimension);
            assert!(!v.iter().all(|&x| x == 0.0));
        }
        Err(e) => {
            panic!(
                "Fallback chain failed to reach working local backend: {}",
                e
            );
        }
    }

    // Cleanup
    std::env::remove_var("XAVIER_EMBEDDING_PROVIDER_MODE");
    std::env::remove_var("XAVIER_EMBEDDING_URL");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("XAVIER_EMBEDDING_LOCAL_URL");
    std::env::remove_var("XAVIER_EMBEDDING_MODEL");

    Ok(())
}

#[tokio::test]
async fn test_noop_embedder() -> Result<()> {
    let noop = NoopEmbedder;
    assert_eq!(noop.dimension(), 0);
    let v = noop.encode("anything").await?;
    assert!(v.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_full_chain_integration() -> Result<()> {
    let dimension = 768;
    let (url, _handle) = start_mock_ollama(dimension).await?;

    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "local");
    std::env::set_var("XAVIER_EMBEDDING_LOCAL_URL", &url);
    std::env::set_var("XAVIER_EMBEDDING_MODEL", "embeddinggemma");

    // 1. Initialize store
    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("test_vec_store.db");
    let config = VecSqliteStoreConfig {
        path: db_path,
        embedding_dimensions: dimension,
    };
    let store = VecSqliteMemoryStore::new(config).await?;

    // 2. Build embedder
    let embedder = build_embedder_from_env().await?;

    // 3. Create a record and manually set embedding (simulating what the system does)
    let content = "El cielo es azul";
    let embedding = embedder.encode(content).await?;

    let record = MemoryRecord {
        id: "test-1".to_string(),
        workspace_id: "ws-1".to_string(),
        path: "test.txt".to_string(),
        content: content.to_string(),
        embedding,
        ..Default::default()
    };

    store.put(record).await?;

    // 4. Search
    let query_embedding = embedder.encode("color del cielo").await?;
    let results = store
        .hybrid_search_with_embedding("ws-1", "cielo", query_embedding, None, 5)
        .await?;

    assert!(!results.is_empty());
    assert_eq!(results[0].record.id, "test-1");
    assert!(results[0].score > 0.0);

    Ok(())
}
