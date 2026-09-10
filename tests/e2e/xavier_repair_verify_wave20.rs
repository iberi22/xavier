//! WAVE-20.05 Final Repair Verification Gate Suite
//!
//! Certifies Wave 20 Repair deliverables:
//! - 20.01: Local deployment script present and syntactically valid.
//! - 20.02: Real memory roundtrip (add document -> search returns hit).
//! - 20.03: Isolated /health fast response (<2s).
//! - 20.04: Bounded memory search execution without hanging.
//! - Stats endpoint integrity (/memory/stats and /v1/stats).

use axum::Extension;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use xavier::{
    agents::RuntimeConfig,
    workspace::{WorkspaceConfig, WorkspaceRegistry, WorkspaceState},
};

/// Helper to generate a unique temp file path for test databases and threads.
fn unique_test_path(prefix: &str, suffix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before UNIX_EPOCH")
        .as_nanos();
    let tid = std::thread::current().id();
    std::env::temp_dir().join(format!("{prefix}-{unique:016x}-{tid:?}-{suffix}"))
}

/// Spawns an ephemeral test HTTP server on an available local port (`127.0.0.1:0`).
/// Returns the base URL (e.g. `http://127.0.0.1:41235`).
async fn spawn_test_server() -> String {
    let unique_id = ulid::Ulid::new().to_string();
    let token = format!("test-token-{unique_id}");
    std::env::set_var("XAVIER_TOKEN", &token);

    let workspace_registry = Arc::new(WorkspaceRegistry::new());

    let workspace_state = WorkspaceState::new(
        WorkspaceConfig {
            id: format!("test-{unique_id}"),
            token: token.clone(),
            plan: xavier::workspace::PlanTier::Personal,
            memory_backend: xavier::memory::store::MemoryBackend::Memory,
            storage_limit_bytes: Some(10 * 1024 * 1024),
            request_limit: Some(10_000),
            request_unit_limit: Some(20_000),
            embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: xavier::workspace::SyncPolicy::CloudMirror,
            dedup: xavier::settings::types::DedupSettings::default(),
        },
        RuntimeConfig::default(),
        unique_test_path(&format!("xavier-wave20-store-{}", unique_id), "threads"),
    )
    .await
    .expect("WorkspaceState creation failed for wave20 test");

    workspace_registry
        .insert(workspace_state)
        .await
        .expect("insert workspace into registry failed");

    let workspace_ctx = workspace_registry
        .authenticate(&token)
        .await
        .expect("authenticate failed for wave20 test workspace");

    let router = xavier::adapters::inbound::http::routes::create_router()
        .route(
            "/v1/memories",
            axum::routing::post(xavier::server::v1_api::v1_memories_add)
                .get(xavier::server::v1_api::v1_memories_list),
        )
        .route(
            "/v1/memories/search",
            axum::routing::post(xavier::server::v1_api::v1_memories_search),
        )
        .route(
            "/memory/stats",
            axum::routing::get(xavier::server::v1_api::v1_memory_recall_stats),
        )
        .route(
            "/v1/stats",
            axum::routing::get(xavier::server::v1_api::v1_memory_recall_stats),
        )
        .layer(Extension(workspace_ctx));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind ephemeral TcpListener");
    let addr = listener
        .local_addr()
        .expect("failed to obtain local SocketAddr");

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("http://{}", addr)
}

/// 1. Health Fast Test (Wave 20.03 certification):
/// Verifies GET /health returns HTTP 200 in less than 2 seconds against a running test server instance.
#[tokio::test(flavor = "multi_thread")]
async fn test_wave20_health_fast() {
    let base_url = spawn_test_server().await;
    let client = reqwest::Client::new();

    let start = Instant::now();
    let resp = client
        .get(format!("{base_url}/health"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .expect("failed to query /health endpoint");

    let elapsed = start.elapsed();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    assert!(
        elapsed < Duration::from_secs(2),
        "health check took too long: {elapsed:?}"
    );

    let body: Value = resp
        .json()
        .await
        .expect("failed to parse /health JSON body");
    assert_eq!(body["service"], "xavier");
    assert!(body["status"].is_string());
}

/// 2. Stats OK Test (Wave 20 Repair certification):
/// Verifies that both `/memory/stats` and `/v1/stats` endpoints respond with valid JSON and status 200 OK.
#[tokio::test(flavor = "multi_thread")]
async fn test_wave20_stats_ok() {
    let base_url = spawn_test_server().await;
    let client = reqwest::Client::new();

    for path in ["/memory/stats", "/v1/stats"] {
        let resp = client
            .get(format!("{base_url}{path}"))
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .expect("failed to query stats endpoint");

        assert_eq!(
            resp.status(),
            reqwest::StatusCode::OK,
            "failed status for {path}"
        );
        let body: Value = resp
            .json()
            .await
            .expect("failed to parse stats response as JSON");
        assert_eq!(body["status"], "ok");
        assert!(body.get("embedding_coverage").is_some());
    }
}

/// 3. Memory Roundtrip Test (Wave 20.02 certification):
/// Inserts a document with a unique string and verifies search returns the document hit.
#[tokio::test(flavor = "multi_thread")]
async fn test_wave20_memory_roundtrip() {
    let base_url = spawn_test_server().await;
    let client = reqwest::Client::new();

    let unique_key = format!("wavecertkey{}", ulid::Ulid::new());
    let memory_content =
        format!("Wave 20 Repair Verification: real embedding roundtrip test for {unique_key} item");

    let add_resp = client
        .post(format!("{base_url}/v1/memories"))
        .json(&serde_json::json!({
            "text": memory_content,
            "path": format!("features/repair/{unique_key}"),
            "kind": "fact"
        }))
        .send()
        .await
        .expect("failed to post memory");

    assert_eq!(add_resp.status(), reqwest::StatusCode::OK);

    let search_resp = client
        .post(format!("{base_url}/v1/memories/search"))
        .json(&serde_json::json!({
            "query": unique_key,
            "limit": 5
        }))
        .send()
        .await
        .expect("failed to execute memory search");

    assert_eq!(search_resp.status(), reqwest::StatusCode::OK);
    let search_body: Value = search_resp
        .json()
        .await
        .expect("failed to parse search response JSON");

    assert_eq!(search_body["status"], "ok");
    let results = search_body["results"]
        .as_array()
        .expect("results field should be an array");
    assert!(
        !results.is_empty(),
        "expected search results to contain added memory for query {unique_key}"
    );

    let found = results
        .iter()
        .any(|item| item["memory"].as_str().unwrap_or("").contains(&unique_key));
    assert!(
        found,
        "roundtrip memory containing {unique_key} was not returned in search results"
    );
}

/// 4. Deploy Script Present Test (Wave 20.01 certification):
/// Verifies `scripts/deploy-local-xavier.sh` exists, is readable, and passes bash syntax validation (`bash -n`).
#[tokio::test(flavor = "multi_thread")]
async fn test_wave20_deploy_script_present() {
    let script_path = Path::new("scripts/deploy-local-xavier.sh");
    assert!(
        script_path.exists(),
        "Hermes deliverable scripts/deploy-local-xavier.sh is missing!"
    );

    let output = Command::new("bash")
        .arg("-n")
        .arg("scripts/deploy-local-xavier.sh")
        .output()
        .expect("failed to run bash -n command");

    assert!(
        output.status.success(),
        "scripts/deploy-local-xavier.sh failed bash syntax check: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// 5. Search Bounded Test (Wave 20.04 certification):
/// Verifies search with timeout bounded execution never hangs (>30s is unacceptable).
#[tokio::test(flavor = "multi_thread")]
async fn test_wave20_search_bounded() {
    let base_url = spawn_test_server().await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("failed to build reqwest client");

    let start = Instant::now();
    let resp = client
        .post(format!("{base_url}/v1/memories/search"))
        .json(&serde_json::json!({
            "query": "bounded timeout test query",
            "limit": 10
        }))
        .send()
        .await
        .expect("search request failed or timed out");

    let elapsed = start.elapsed();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    assert!(
        elapsed < Duration::from_secs(30),
        "search execution exceeded bounded threshold: {elapsed:?}"
    );
}
