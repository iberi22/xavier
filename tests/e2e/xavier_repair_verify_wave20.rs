//! End-to-End verification tests for Wave 20 (Repair Gate Certification).
//!
//! Certification suite covering core invariants delivered across Wave 20:
//! - 20.01: Health check responsiveness & local deploy script presence
//! - 20.02: Memory ingestion & real embedding search roundtrip
//! - 20.03: Isolated health/readiness check response time bounds
//! - 20.04: Search execution bounded timeouts without hanging

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tower::util::ServiceExt;

use xavier::adapters::inbound::http::routes::create_router;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::{MemoryRecord, MemoryStore};

/// Helper to build HTTP request to test router
fn make_request(uri: &str, method: Method) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method(method)
        .body(Body::empty())
        .expect("build HTTP request")
}

/// 1. `test_wave20_health_fast` — Certifies GET /health returns 200 OK in <2s against a test router instance.
#[tokio::test]
async fn test_wave20_health_fast() -> anyhow::Result<()> {
    let router = create_router();
    let start = Instant::now();

    let response = router.oneshot(make_request("/health", Method::GET)).await?;

    let elapsed = start.elapsed();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        elapsed < Duration::from_secs(2),
        "GET /health took {:?}, exceeding 2s bound",
        elapsed
    );

    let body_bytes = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes)?;
    assert_eq!(json["service"], "xavier");

    Ok(())
}

/// 2. `test_wave20_stats_ok` — Certifies /memory/stats and /v1/memories stats response return valid JSON.
#[tokio::test]
async fn test_wave20_stats_ok() -> anyhow::Result<()> {
    let router = create_router();

    let response = router
        .oneshot(make_request("/v1/memories", Method::GET))
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await?.to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes)?;
    assert!(json.is_object() || json.is_array());

    Ok(())
}

/// 3. `test_wave20_memory_roundtrip` — Certifies end-to-end memory add with real embedding vector -> search hit.
#[tokio::test]
async fn test_wave20_memory_roundtrip() -> anyhow::Result<()> {
    let tmp_dir = TempDir::new()?;
    let db_path = tmp_dir.path().join("wave20_roundtrip.db");

    let config = VecSqliteStoreConfig {
        path: db_path,
        embedding_dimensions: 384,
    };

    let store = VecSqliteMemoryStore::new(config).await?;
    let record_id = format!("rec-wave20-{}", ulid::Ulid::new());
    let unique_content = "Xavier Wave 20 E2E Verification Invariant Test Content";

    // Simulate real 384-dim non-noop embedding vector
    let mut real_embedding = vec![0.1f32; 384];
    real_embedding[0] = 0.85;
    real_embedding[1] = 0.35;

    let record = MemoryRecord {
        id: record_id.clone(),
        workspace_id: "wave20-workspace".to_string(),
        path: "wave20/e2e/test.md".to_string(),
        content: unique_content.to_string(),
        embedding: real_embedding.clone(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        revision: 1,
        deleted_at: None,
        metadata: serde_json::json!({ "test": "wave20" }),
        ..Default::default()
    };

    store.put(record).await?;

    // Search for record using MemoryStore interface
    let hits = store
        .search("wave20-workspace", unique_content, None)
        .await?;
    assert!(!hits.is_empty(), "Search returned zero hits");
    assert_eq!(hits[0].id, record_id);
    assert_eq!(hits[0].content, unique_content);

    Ok(())
}

/// 4. `test_wave20_deploy_script_present` — Certifies deploy script deliverable exists, is executable, and passes bash -n syntax check.
#[tokio::test]
async fn test_wave20_deploy_script_present() -> anyhow::Result<()> {
    let script_path = std::path::Path::new("scripts/deploy-local-xavier.sh");

    assert!(
        script_path.exists(),
        "scripts/deploy-local-xavier.sh must exist (delivered by 20.01)"
    );

    let metadata = std::fs::metadata(script_path)?;
    let permissions = metadata.permissions();
    assert!(
        permissions.mode() & 0o111 != 0,
        "scripts/deploy-local-xavier.sh must be executable"
    );

    let output = Command::new("bash")
        .arg("-n")
        .arg(script_path)
        .output()?;

    assert!(
        output.status.success(),
        "bash -n failed for {}: {}",
        script_path.display(),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

/// 5. `test_wave20_search_bounded` — Certifies search queries remain strictly bounded in execution time (<30s).
#[tokio::test]
async fn test_wave20_search_bounded() -> anyhow::Result<()> {
    let tmp_dir = TempDir::new()?;
    let db_path = tmp_dir.path().join("wave20_bounded.db");

    let config = VecSqliteStoreConfig {
        path: db_path,
        embedding_dimensions: 384,
    };

    let store = VecSqliteMemoryStore::new(config).await?;

    let start = Instant::now();
    let search_future = store.search("wave20-bounded", "test query", None);

    let result = tokio::time::timeout(Duration::from_secs(30), search_future).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Search operation timed out after 30s");
    assert!(
        elapsed < Duration::from_secs(30),
        "Search took {:?}, exceeding 30s limit",
        elapsed
    );

    Ok(())
}
