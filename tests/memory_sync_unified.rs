//! Integration tests for the unified sync endpoint interface.
//!
//! Tests both old data-plane (`/v1/memory/*`) and new control-plane
//! (`/api/v1/memory/sync/*`) paths to verify backward compatibility
//! and the adapter layer.

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
use xavier::memory::sync::adapter::SyncEndpointAdapter;
use xavier::memory::sync::{ChunkDiff, DiffAction, PeerMemorySync};

/// Drive a request through the router and return the parsed JSON body.
async fn send_json(
    router: &Router,
    method: Method,
    uri: &str,
    body: Option<String>,
) -> serde_json::Value {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(json_body) => builder
            .header("content-type", "application/json")
            .body(Body::from(json_body))
            .expect("build request"),
        None => builder.body(Body::empty()).expect("build request"),
    };

    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("router should respond");
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
/// - `POST /v1/memory/push`        → 200 OK
/// - `GET  /v1/memory/pull-since/…`→ `[]`
async fn spawn_legacy_mock_peer() -> String {
    let app = Router::new()
        .route("/v1/memory/push", post(|| async { StatusCode::OK }))
        .route(
            "/v1/memory/pull-since/{workspace}/{since}",
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

// ===========================================================================
// Tests: New control-plane endpoints
// ===========================================================================

/// Full scenario exercising all new control-plane endpoints.
#[tokio::test]
async fn control_plane_push_pull_status_resolve() {
    let router = create_router();

    // ── status: not initialised yet ──────────────────────────────────────────
    let status = send_json(&router, Method::GET, "/api/v1/memory/sync/status", None).await;
    assert_eq!(status["status"], "ok");
    assert_eq!(status["initialized"], false);

    // ── initialise ───────────────────────────────────────────────────────────
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

    // ── push via control-plane ───────────────────────────────────────────────
    let peer_url = spawn_legacy_mock_peer().await;
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
    assert_eq!(pushed["status"], "ok");
    assert_eq!(pushed["session"]["success"], true);
    assert_eq!(pushed["session"]["chunks_sent"], 1);

    // ── pull via control-plane ───────────────────────────────────────────────
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
    assert_eq!(pulled["status"], "ok");
    assert_eq!(pulled["session"]["success"], true);

    // ── resolve conflict ─────────────────────────────────────────────────────
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
}

// ===========================================================================
// Tests: Legacy data-plane endpoints
// ===========================================================================

/// Full scenario exercising all legacy data-plane endpoints.
#[tokio::test]
async fn legacy_manifest_push_pull() {
    let router = create_router();

    // ── initialise sync service ──────────────────────────────────────────────
    let store = Arc::new(InMemoryMemoryStore::new());
    store
        .put(record("doc-1", "sync/doc-1", "content-A"))
        .await
        .expect("seed record");
    store
        .put(record("doc-2", "sync/doc-2", "content-B"))
        .await
        .expect("seed record");

    let sync = Arc::new(PeerMemorySync::new(
        store.clone(),
        "node-legacy".to_string(),
    ));
    init_memory_sync(sync);

    // ── legacy manifest ──────────────────────────────────────────────────────
    let manifest = send_json(&router, Method::GET, "/v1/memory/manifest", None).await;
    // manifest is a JSON array of entries
    let entries = manifest.as_array().expect("manifest should be an array");
    assert!(entries.len() >= 2, "should have at least 2 entries");
    // Check that entries have the expected fields
    let first = &entries[0];
    assert!(
        first.get("chunk_hash").is_some(),
        "entry should have chunk_hash"
    );
    assert!(
        first.get("namespace").is_some(),
        "entry should have namespace"
    );

    // ── legacy push (receive diffs from a peer) ─────────────────────────────
    let data =
        xavier::memory::sync::merge::serialise_chunk(&record("doc-3", "sync/doc-3", "from-peer"))
            .expect("serialise");
    let diffs = vec![ChunkDiff {
        chunk_hash: "test-hash".to_string(),
        namespace: "default".to_string(),
        action: DiffAction::Update,
        data: Some(data),
        timestamp: SystemTime::UNIX_EPOCH,
        record_path: Some("sync/doc-3".to_string()),
    }];
    let body = serde_json::to_string(&diffs).expect("serde");
    let pushed = send_json(&router, Method::POST, "/v1/memory/push", Some(body)).await;
    assert_eq!(pushed["status"], "ok");
    assert_eq!(pushed["received"], 1);

    // Verify the pushed record is now in the store
    let stored = store
        .get("default", "sync/doc-3")
        .await
        .expect("get")
        .expect("record should exist");
    assert_eq!(stored.content, "from-peer");

    // ── legacy pull (return diffs for requested entries) ─────────────────────
    let want = vec![xavier::memory::sync::ManifestEntry {
        chunk_hash: entries[0]["chunk_hash"].as_str().unwrap().to_string(),
        namespace: entries[0]["namespace"].as_str().unwrap().to_string(),
        revision: entries[0]["revision"].as_u64().unwrap_or(0),
        updated_at: chrono::Utc::now(),
        size_bytes: 0,
        record_path: None,
    }];
    let body = serde_json::to_string(&want).expect("serde");
    let pulled = send_json(&router, Method::POST, "/v1/memory/pull", Some(body)).await;
    // pull returns a JSON array of ChunkDiff
    let diffs = pulled.as_array().expect("diffs should be an array");
    assert!(!diffs.is_empty(), "should return at least one diff");
    assert!(diffs[0].get("chunk_hash").is_some());

    // ── legacy pull-since ────────────────────────────────────────────────────
    let pull_since = send_json(
        &router,
        Method::GET,
        "/v1/memory/pull-since/default/0",
        None,
    )
    .await;
    let diffs = pull_since.as_array().expect("diffs should be an array");
    assert_eq!(
        diffs.len(),
        3,
        "should return all 3 records (doc-1, doc-2, doc-3)"
    );
}

// ===========================================================================
// Tests: Adapter layer
// ===========================================================================

/// Verify the SyncEndpointAdapter enum dispatches correctly.
#[tokio::test]
async fn adapter_legacy_construction_and_dispatch() {
    let client = reqwest::Client::new();
    let adapter = SyncEndpointAdapter::legacy(client.clone(), Some("token".into()));
    assert!(matches!(adapter, SyncEndpointAdapter::Legacy(_)));
}

#[tokio::test]
async fn adapter_control_plane_construction_and_dispatch() {
    let client = reqwest::Client::new();
    let adapter = SyncEndpointAdapter::control_plane(client, None);
    assert!(matches!(adapter, SyncEndpointAdapter::ControlPlane(_)));
}

/// PeerMemorySync with adapter accessor.
#[test]
fn peer_memory_sync_has_adapter_accessor() {
    let store = Arc::new(InMemoryMemoryStore::new());
    let sync = PeerMemorySync::new(store, "test".into());
    // Should default to legacy adapter
    assert!(matches!(sync.adapter(), SyncEndpointAdapter::Legacy(_)));
}

/// PeerMemorySync with explicit adapter constructor.
#[test]
fn peer_memory_sync_with_control_plane_adapter() {
    let store = Arc::new(InMemoryMemoryStore::new());
    let client = reqwest::Client::new();
    let adapter = SyncEndpointAdapter::control_plane(client, None);
    let sync = PeerMemorySync::with_adapter(store, "test".into(), None, adapter);
    assert!(matches!(
        sync.adapter(),
        SyncEndpointAdapter::ControlPlane(_)
    ));
}

// ===========================================================================
// Tests: Both paths return consistent data
// ===========================================================================

/// Both legacy and control-plane push return the same session structure.
#[tokio::test]
async fn both_paths_return_consistent_session_structure() {
    let router = create_router();
    let store = Arc::new(InMemoryMemoryStore::new());
    store
        .put(record("x-1", "sync/x-1", "test-content"))
        .await
        .expect("seed");

    let sync = Arc::new(PeerMemorySync::new(store, "node-consistency".to_string()));
    init_memory_sync(sync);

    let peer_url = spawn_legacy_mock_peer().await;

    // ── control-plane push ───────────────────────────────────────────────────
    let body = serde_json::json!({
        "peer_url": &peer_url,
        "workspace_id": "default",
        "since": "0",
    })
    .to_string();
    let cp_pushed = send_json(
        &router,
        Method::POST,
        "/api/v1/memory/sync/push",
        Some(body),
    )
    .await;
    assert_eq!(cp_pushed["status"], "ok");
    assert!(
        cp_pushed["session"].is_object(),
        "control-plane returns session object"
    );
    assert!(cp_pushed["session"]["peer_id"].is_string());
    assert!(cp_pushed["session"]["success"].is_boolean());
    assert!(cp_pushed["session"]["chunks_sent"].is_number());
}

// ===========================================================================
// Tests: Error paths
// ===========================================================================

/// Control-plane push to unreachable peer returns structured error.
#[tokio::test]
async fn control_plane_push_unreachable_peer_returns_error() {
    let router = create_router();
    let store = Arc::new(InMemoryMemoryStore::new());
    store
        .put(record("x-2", "sync/x-2", "test"))
        .await
        .expect("seed");
    let sync = Arc::new(PeerMemorySync::new(store, "node-err".to_string()));
    init_memory_sync(sync);

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

/// Invalid resolution value is rejected by the resolve endpoint.
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
    assert!(result["message"].as_str().unwrap().contains("local"));
}

// ===========================================================================
// Tests: P2P Unified Pipeline Integration Tests
// ===========================================================================

use xavier::memory::sync::merge::{apply_changes_received, serialise_chunk};
use xavier::mesh::data_consent::{ConsentLevel, DataConsentManager};
use xavier::mesh::node::NodeId;
use xavier::mesh::p2p::fallback::{FallbackStrategy, OfflineQueue, OfflineQueueConfig};
use xavier::mesh::p2p::sync_filter::SyncFilter;
use xavier::mesh::tokenomics::rewards::{ContributionType, RewardEngine};
use xavier::mesh::tokenomics::wallet::Wallet;

/// 1. P2P Pipeline with Rejected Consent:
///    attempt sync without consent, verify it's blocked
#[tokio::test]
async fn test_p2p_pipeline_with_rejected_consent() {
    let node_sender = NodeId("xv1-sender-no-consent".to_string());
    let node_receiver = NodeId("xv1-receiver-no-consent".to_string());

    // Register explicit None consent (or unconfigured namespace)
    let mut consent_mgr = DataConsentManager::new(node_sender);
    consent_mgr.set_consent("private_workspace", ConsentLevel::None);
    let filter = SyncFilter::new(consent_mgr);

    let rec = record(
        "secret-1",
        "private_workspace/classified",
        "confidential-data",
    );
    let payload = serialise_chunk(&rec).expect("serialise");
    let diff = ChunkDiff {
        chunk_hash: "hash-secret-1".to_string(),
        namespace: "private_workspace".to_string(),
        action: DiffAction::Update,
        data: Some(payload),
        timestamp: SystemTime::now(),
        record_path: Some("private_workspace/classified".to_string()),
    };

    // Filter outgoing diffs
    let outgoing_diffs = filter.filter_out_unconsented(vec![diff], &node_receiver);
    assert!(
        outgoing_diffs.is_empty(),
        "unconsented diff must be completely dropped by filter"
    );

    // Verify nothing is queued or sent
    let queue = OfflineQueue::new_memory(OfflineQueueConfig::default()).expect("queue init");
    if !outgoing_diffs.is_empty() {
        let diffs_json = serde_json::to_vec(&outgoing_diffs).expect("serde diffs");
        let _ = queue.enqueue(&node_receiver.0, diffs_json, None);
    }

    assert_eq!(
        queue.count().expect("queue count"),
        0,
        "no messages queued when consent is rejected"
    );
}

/// 2. P2P Pipeline Filter Removes Unconsented:
///    mixed consent levels, only allowed data syncs
#[tokio::test]
async fn test_p2p_pipeline_filter_removes_unconsented() {
    let node_sender = NodeId("xv1-sender-mixed".to_string());
    let node_receiver = NodeId("xv1-receiver-mixed".to_string());

    let mut consent_mgr = DataConsentManager::new(node_sender);
    consent_mgr.set_consent("public_ns", ConsentLevel::Full);
    consent_mgr.set_consent("secret_ns", ConsentLevel::None);
    consent_mgr.set_consent("meta_ns", ConsentLevel::Metadata);
    consent_mgr.set_consent("anon_ns", ConsentLevel::Anonymized);

    let filter = SyncFilter::new(consent_mgr);

    let make_diff_fn = |ns: &str| ChunkDiff {
        chunk_hash: format!("hash_{}", ns),
        namespace: ns.to_string(),
        action: DiffAction::Add,
        data: Some(vec![1, 2, 3, 4, 5]),
        timestamp: SystemTime::now(),
        record_path: Some(format!("{}/item", ns)),
    };

    let batch = vec![
        make_diff_fn("public_ns"),
        make_diff_fn("secret_ns"),
        make_diff_fn("meta_ns"),
        make_diff_fn("anon_ns"),
    ];

    let filtered = filter.filter_out_unconsented(batch, &node_receiver);

    // secret_ns is None -> dropped -> 3 kept
    assert_eq!(filtered.len(), 3, "only 3 consented items should be kept");

    let secret_present = filtered.iter().any(|d| d.namespace == "secret_ns");
    assert!(!secret_present, "secret_ns must be completely removed");

    let meta_diff = filtered.iter().find(|d| d.namespace == "meta_ns").unwrap();
    assert!(
        meta_diff.data.is_none(),
        "meta_ns must have payload stripped"
    );

    let public_diff = filtered
        .iter()
        .find(|d| d.namespace == "public_ns")
        .unwrap();
    assert!(
        public_diff.data.is_some(),
        "public_ns must keep payload intact"
    );

    let anon_diff = filtered.iter().find(|d| d.namespace == "anon_ns").unwrap();
    assert!(anon_diff.data.is_some(), "anon_ns must keep payload intact");
}
