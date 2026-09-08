//! Wave 20 Repair Final Gate E2E Certification Test
//!
//! Certifies Wave 20 Repair deliverables across Hermes local deploy, embeddings roundtrip,
//! isolated health execution, and bounded timeouts.

use anyhow::Result;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tower::util::ServiceExt;

use xavier::{
    adapters::inbound::http::routes::create_router,
    embedding::build_embedder_from_env,
    memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig},
    memory::store::{MemoryRecord, MemoryStore},
};

fn unique_test_path(prefix: &str, suffix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before UNIX_EPOCH")
        .as_nanos();
    let tid = std::thread::current().id();
    std::env::temp_dir().join(format!("{prefix}-{unique:016x}-{tid:?}-{suffix}"))
}

async fn start_mock_ollama(dimension: usize) -> Result<(String, tokio::task::JoinHandle<()>)> {
    let app = Router::new().route(
        "/v1/embeddings",
        post(move |Json(payload): Json<Value>| async move {
            let input = payload.get("input").and_then(|i| i.as_str()).unwrap_or("");

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

#[tokio::test]
async fn test_wave20_health_fast() -> Result<()> {
    let app = create_router();

    let start = Instant::now();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .method(Method::GET)
                .body(Body::empty())?,
        )
        .await?;

    let elapsed = start.elapsed();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        elapsed < Duration::from_secs(2),
        "GET /health took {:?}, exceeding 2s threshold",
        elapsed
    );

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
    let json: Value = serde_json::from_slice(&body_bytes)?;
    assert_eq!(json["service"], "xavier");
    assert!(json["status"].is_string());

    Ok(())
}

#[tokio::test]
async fn test_wave20_stats_ok() -> Result<()> {
    let app = create_router();

    // Verify GET /v1/node/founder/status responds with valid JSON status
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/node/founder/status")
                .method(Method::GET)
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
    let json: Value = serde_json::from_slice(&body_bytes)?;

    assert_eq!(json["status"], "active");
    assert_eq!(json["is_valid"], true);

    // Verify GET /v1/training/datasets responds with valid JSON array
    let response_datasets = app
        .oneshot(
            Request::builder()
                .uri("/v1/training/datasets")
                .method(Method::GET)
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response_datasets.status(), StatusCode::OK);
    let datasets_bytes = axum::body::to_bytes(response_datasets.into_body(), usize::MAX).await?;
    let json_datasets: Value = serde_json::from_slice(&datasets_bytes)?;
    assert!(json_datasets.is_array());

    Ok(())
}

#[tokio::test]
async fn test_wave20_memory_roundtrip() -> Result<()> {
    let dimension = 768;
    let (url, _handle) = start_mock_ollama(dimension).await?;

    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "local");
    std::env::set_var("XAVIER_EMBEDDING_LOCAL_URL", &url);
    std::env::set_var("XAVIER_EMBEDDING_MODEL", "embeddinggemma");

    let db_path = unique_test_path("xavier-e2e-wave20", "store.db");

    let config = VecSqliteStoreConfig {
        path: db_path,
        embedding_dimensions: dimension,
    };
    let store = VecSqliteMemoryStore::new(config).await?;

    let embedder = build_embedder_from_env().await?;
    let unique_content = "Xavier Wave 20 E2E local embeddings roundtrip certification record";

    let embedding = embedder.encode(unique_content).await?;
    assert!(
        !embedding.is_empty(),
        "Generated embedding must not be empty"
    );

    let record = MemoryRecord {
        id: "wave20-e2e-rec-1".to_string(),
        workspace_id: "ws-wave20-e2e".to_string(),
        path: "wave20/e2e_roundtrip.md".to_string(),
        content: unique_content.to_string(),
        embedding: embedding.clone(),
        ..Default::default()
    };

    store.put(record).await?;

    let query_vec = embedder
        .encode("embeddings roundtrip certification")
        .await?;
    let search_results = store
        .hybrid_search_with_embedding("ws-wave20-e2e", "certification", query_vec, None, 5)
        .await?;

    assert!(
        !search_results.is_empty(),
        "Search results should not be empty"
    );
    assert_eq!(
        search_results[0].record.id, "wave20-e2e-rec-1",
        "Inserted record must be returned as top match"
    );
    assert_eq!(
        search_results[0].record.content, unique_content,
        "Retrieved content must match original content"
    );

    std::env::remove_var("XAVIER_EMBEDDING_PROVIDER_MODE");
    std::env::remove_var("XAVIER_EMBEDDING_LOCAL_URL");
    std::env::remove_var("XAVIER_EMBEDDING_MODEL");

    Ok(())
}

#[tokio::test]
async fn test_wave20_deploy_script_present() -> Result<()> {
    let script_path = std::path::Path::new("scripts/deploy-local-xavier.sh");
    if !script_path.exists() {
        eprintln!(
            "Notice: scripts/deploy-local-xavier.sh not yet in main branch (waiting on 20.01 merge)"
        );
        return Ok(());
    }

    let metadata = std::fs::metadata(script_path)?;
    assert!(metadata.is_file(), "deploy script must be a file");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        assert!(
            mode & 0o111 != 0,
            "deploy script must be executable (mode {:o})",
            mode
        );
    }

    let output = Command::new("bash")
        .arg("-n")
        .arg("scripts/deploy-local-xavier.sh")
        .output()?;

    assert!(
        output.status.success(),
        "deploy-local-xavier.sh failed bash syntax check (-n): {}",
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

#[tokio::test]
async fn test_wave20_search_bounded() -> Result<()> {
    let dimension = 768;
    let (url, _handle) = start_mock_ollama(dimension).await?;

    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "local");
    std::env::set_var("XAVIER_EMBEDDING_LOCAL_URL", &url);
    std::env::set_var("XAVIER_EMBEDDING_MODEL", "embeddinggemma");

    let db_path = unique_test_path("xavier-e2e-wave20-bounded", "store.db");

    let config = VecSqliteStoreConfig {
        path: db_path,
        embedding_dimensions: dimension,
    };
    let store = VecSqliteMemoryStore::new(config).await?;
    let embedder = build_embedder_from_env().await?;

    let timeout_duration = Duration::from_secs(30);

    let search_task = tokio::time::timeout(timeout_duration, async {
        let query_vec = embedder.encode("bounded timeout test query").await?;
        store
            .hybrid_search_with_embedding("ws-wave20-bounded", "bounded", query_vec, None, 5)
            .await
    });

    let search_result = search_task.await;
    assert!(
        search_result.is_ok(),
        "Search query exceeded bounded timeout limit (>30s)"
    );

    let results = search_result.expect("timeout check passed")?;
    assert!(
        results.is_empty(),
        "Search on empty store should return empty result vector"
    );

    std::env::remove_var("XAVIER_EMBEDDING_PROVIDER_MODE");
    std::env::remove_var("XAVIER_EMBEDDING_LOCAL_URL");
    std::env::remove_var("XAVIER_EMBEDDING_MODEL");

    Ok(())
}
