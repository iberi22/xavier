//! Integration tests for the Memory Sync HTTP endpoints (#193).
//!
//! Covers all four endpoints registered in `adapters/inbound/http/routes.rs`:
//!   - `POST /api/v1/memory/sync/push`
//!   - `POST /api/v1/memory/sync/pull`
//!   - `GET  /api/v1/memory/sync/status`
//!   - `POST /api/v1/memory/sync/resolve/{conflict_id}`
//!
//! Push/pull happy paths are exercised against a tiny in-process axum mock peer
//! that speaks the `/v1/memory/push` and `/v1/memory/pull-since/...` contract
//! the `PeerMemorySync` client expects.

use std::sync::Arc;
use std::time::SystemTime;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    routing::{get, post},
    Json, Router,
};
use http_body_util::BodyExt;
use tower::util::ServiceExt;

use xavier::adapters::inbound::http::handlers::sync::init_memory_sync;
use xavier::adapters::inbound::http::routes::create_router;
use xavier::memory::store::{InMemoryMemoryStore, MemoryRecord, MemoryStore};
use xavier::memory::sync::merge::serialise_chunk;
use xavier::memory::sync::{ChunkDiff, DiffAction, PeerMemorySync};

/// Drive a request through the router and return the parsed JSON body.
async fn send_json(
    router: &Router,
    method: Method,
    uri: &str,
    body: Option<String>,
) -> serde_json::Value {
    let mut builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(json_body) => builder
            .header("content-type", "application/json")
            .body(Body::from(json_body))
            .expect("build request"),
        None => {
            builder.body(Body::empty()).expect("build request")
        }
    };

    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("router should respond");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "unexpected status for {uri}"
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response should be valid JSON")
}

/// Spawn a minimal mock peer that satisfies the `PeerMemorySync` client.
///
/// - `POST /v1/memory/push`        → 200 OK (accepts anything)
/// - `GET  /v1/memory/pull-since/…`→ `[]`   (no remote changes)
async fn spawn_mock_peer() -> String {
    let app = Router::new()
        .route(
            "/v1/memory/push",
            post(|| async { StatusCode::OK }),
        )
        .route(
            "/v1/memory/pull-since/{workspace}/{since}",
            // Empty diff list — pull becomes a successful no-op.
            get(|| async { Json(Vec::<serde_json::Value>::new()) }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock peer");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

fn record(id: &str, path: &str, content: &str) -> MemoryRecord {
    let now = chrono::Utc::now();
    MemoryRecord {
        id: id.to_string(),
        workspace_id: "default".to_string(),
        path: path.to_string(),
        content: content.to_string(),
        created_at: now,
        updated_at: now,
        revision: 1,
        ..Default::default()
    }
}

/// Comprehensive scenario. All global-mutating steps live in this single test
/// to avoid races between parallel tests sharing the module singleton.
#[tokio::test]
async fn memory_sync_endpoints_full_scenario() {
    let router = create_router();

    // ── status: not initialised yet ──────────────────────────────────────────
    let status = send_json(&router, Method::GET, "/api/v1/memory/sync/status", None).await;
    assert_eq!(status["status"], "ok");
    assert_eq!(status["initialized"], false, "sync not initialised yet");
    assert!(status["last_session"].is_null());

    // ── initialise with an in-memory store holding one record ────────────────
    let store = Arc::new(InMemoryMemoryStore::new());
    store
        .put(record("doc-1", "sync/doc-1", "local-content"))
        .await
        .expect("seed record");

    let sync = Arc::new(PeerMemorySync::new(store.clone(), "node-test".to_string()));
    init_memory_sync(sync);

    let status = send_json(&router, Method::GET, "/api/v1/memory/sync/status", None).await;
    assert_eq!(status["initialized"], true);
    assert_eq!(status["node_id"], "node-test");
    assert_eq!(status["sync_interval_secs"], 300);

    // ── push (happy path) against the mock peer ──────────────────────────────
    let peer_url = spawn_mock_peer().await;
    let body = serde_json::json!({
        "peer_url": peer_url,
        "workspace_id": "default",
        "since": "0",
    })
    .to_string();
    let pushed = send_json(
        &router,
        Method::POST,
        "/api/v1/memory/sync/push",
        Some(body),
    )
    .await;
    assert_eq!(pushed["status"], "ok", "push should succeed: {pushed}");
    assert_eq!(pushed["session"]["success"], true);
    assert_eq!(
        pushed["session"]["chunks_sent"], 1,
        "the single seeded record should be pushed"
    );
    assert_eq!(pushed["session"]["peer_id"], peer_url);

    // ── pull (happy path) — peer reports no changes ──────────────────────────
    let body = serde_json::json!({
        "peer_url": peer_url,
        "workspace_id": "default",
        "since": "0",
    })
    .to_string();
    let pulled = send_json(
        &router,
        Method::POST,
        "/api/v1/memory/sync/pull",
        Some(body),
    )
    .await;
    assert_eq!(pulled["status"], "ok", "pull should succeed: {pulled}");
    assert_eq!(pulled["session"]["success"], true);
    assert_eq!(pulled["session"]["chunks_received"], 0);

    // ── status now reflects the last session ─────────────────────────────────
    let status = send_json(&router, Method::GET, "/api/v1/memory/sync/status", None).await;
    assert!(
        status["last_session"].is_object(),
        "last_session should be populated after a sync"
    );
    assert_eq!(status["last_session"]["success"], true);

    // ── resolve: local (keeps our record) ────────────────────────────────────
    let body = serde_json::json!({ "resolution": "local" }).to_string();
    let resolved = send_json(
        &router,
        Method::POST,
        "/api/v1/memory/sync/resolve/conflict-A",
        Some(body),
    )
    .await;
    assert_eq!(resolved["status"], "resolved");
    assert_eq!(resolved["conflict_id"], "conflict-A");
    assert_eq!(resolved["resolution"], "local");
    assert_eq!(resolved["applied"], false);

    // ── resolve: remote — force-apply the provided chunk to the store ────────
    let remote_record = record("doc-1", "sync/doc-1", "resolved-remote-content");
    let data = serialise_chunk(&remote_record).expect("serialise chunk");
    let chunk = serde_json::to_value(ChunkDiff {
        chunk_hash: "ignored".to_string(),
        namespace: "default".to_string(),
        action: DiffAction::Update,
        data: Some(data),
        timestamp: SystemTime::UNIX_EPOCH,
    })
    .expect("serialise ChunkDiff");
    let body = serde_json::json!({ "resolution": "remote", "chunk": chunk }).to_string();
    let resolved = send_json(
        &router,
        Method::POST,
        "/api/v1/memory/sync/resolve/conflict-B",
        Some(body),
    )
    .await;
    assert_eq!(resolved["status"], "resolved");
    assert_eq!(resolved["conflict_id"], "conflict-B");
    assert_eq!(resolved["resolution"], "remote");
    assert_eq!(resolved["applied"], true, "remote chunk should be applied");

    // The store must now hold the forced remote value.
    let stored = store
        .get("default", "sync/doc-1")
        .await
        .expect("get")
        .expect("record present");
    assert_eq!(stored.content, "resolved-remote-content");

    // ── status lists both resolved conflicts (idempotent) ────────────────────
    let status = send_json(&router, Method::GET, "/api/v1/memory/sync/status", None).await;
    let resolved_list = status["resolved_conflicts"]
        .as_array()
        .expect("resolved_conflicts array");
    assert_eq!(resolved_list.len(), 2);
    assert!(resolved_list.iter().any(|v| v == "conflict-A"));
    assert!(resolved_list.iter().any(|v| v == "conflict-B"));

    // Re-resolving conflict-A must not duplicate the entry.
    let body = serde_json::json!({ "resolution": "local" }).to_string();
    let _ = send_json(
        &router,
        Method::POST,
        "/api/v1/memory/sync/resolve/conflict-A",
        Some(body),
    )
    .await;
    let status = send_json(&router, Method::GET, "/api/v1/memory/sync/status", None).await;
    let resolved_list = status["resolved_conflicts"]
        .as_array()
        .expect("resolved_conflicts array");
    assert_eq!(resolved_list.len(), 2, "resolution should be idempotent");

    // ── push against an unreachable peer returns a structured error ──────────
    // (Port 1 on localhost is closed → connection refused immediately, so this
    // exercises the transport error path without a long timeout.)
    let body = serde_json::json!({
        "peer_url": "http://127.0.0.1:1",
        "workspace_id": "default",
        "since": "0",
    })
    .to_string();
    let result = send_json(
        &router,
        Method::POST,
        "/api/v1/memory/sync/push",
        Some(body),
    )
    .await;
    assert_eq!(result["status"], "error");
    assert_eq!(result["peer_url"], "http://127.0.0.1:1");
}

/// Invalid resolution values are rejected without touching the global state.
/// (Parallel-safe: this path returns before reading any singleton.)
#[tokio::test]
async fn resolve_rejects_invalid_resolution() {
    let router = create_router();

    let body = serde_json::json!({ "resolution": "bogus" }).to_string();
    let result = send_json(
        &router,
        Method::POST,
        "/api/v1/memory/sync/resolve/conflict-X",
        Some(body),
    )
    .await;

    assert_eq!(result["status"], "error");
    assert_eq!(result["conflict_id"], "conflict-X");
    assert!(
        result["message"]
            .as_str()
            .unwrap_or_default()
            .contains("local"),
        "should mention allowed values: {result}"
    );
}
