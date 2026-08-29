//! Multi-Platform Ecosystem Verification Suite (`tests/e2e_ecosystem_verification.rs`)
//!
//! Validates contracts across Web APIs, Desktop/Headless mode, Memory subsystem,
//! and Mesh P2P offline buffering & consent filters.

use axum::{extract::Json, routing::get, Router};
use chrono::Utc;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::net::TcpListener;
use xavier::governance::quadratic_voting::{
    calculate_effective_votes, IvnIdentityTier, QuadraticVoteEngine,
};
use xavier::humanchallenge::SessionScanner;
use xavier::memory::compression::semantic_compressor::{
    DialogueTurn, SemanticCompressor, SemanticCompressorConfig,
};
use xavier::mesh::data_consent::{ConsentLevel, DataConsentManager};
use xavier::mesh::node::NodeId;
use xavier::mesh::p2p::fallback::{OfflineQueue, OfflineQueueConfig};
use xavier::mesh::p2p::sync_filter::SyncFilter;
use xavier::server::maloca::data_node::{
    get_consent_handler, update_consent_handler, ConsentUpdateRequest, DataNodeConsentResponse,
    DataNodeManager,
};
use xavier::session::types::{SessionEvent, SessionEventType};

#[derive(Debug, Serialize, Deserialize)]
struct MockRegistryResponse {
    apps: Vec<Value>,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MockAlignmentResponse {
    score: u32,
    compliant: bool,
    goals: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MockHeadlessHealthResponse {
    status: String,
    service: String,
    mode: String,
}

struct EcosystemTestServer {
    base_url: String,
    _temp_dir: TempDir,
}

async fn mock_registry_handler() -> Json<MockRegistryResponse> {
    Json(MockRegistryResponse {
        apps: vec![
            json!({"id": "maloca-ui", "name": "Maloca UI", "tier": "desktop"}),
            json!({"id": "xavier-headless", "name": "Headless Memory Runtime", "tier": "headless"}),
            json!({"id": "swal-web-embed", "name": "SWAL Web Component", "tier": "web"}),
        ],
        status: "ok".to_string(),
    })
}

async fn mock_alignment_handler() -> Json<MockAlignmentResponse> {
    let goals: Vec<Value> = (1..=12)
        .map(|i| json!({"goal_id": format!("GOAL-{}", i), "compliant": true}))
        .collect();
    Json(MockAlignmentResponse {
        score: 100,
        compliant: true,
        goals,
    })
}

async fn mock_headless_health_handler() -> Json<MockHeadlessHealthResponse> {
    Json(MockHeadlessHealthResponse {
        status: "ok".to_string(),
        service: "xavier-headless-verifier".to_string(),
        mode: "desktop_headless_api".to_string(),
    })
}

async fn spawn_ecosystem_test_server() -> EcosystemTestServer {
    let temp_dir = TempDir::new().expect("create temp dir for ecosystem server");
    let consent_path = temp_dir.path().join("datanode_consent.json");

    let manager = DataNodeManager::default().with_file_path(consent_path);

    let maloca_router = Router::new()
        .route("/registry", get(mock_registry_handler))
        .route("/alignment", get(mock_alignment_handler))
        .route(
            "/node/consent",
            get(get_consent_handler).post(update_consent_handler),
        )
        .with_state(manager);

    let headless_router = Router::new().route("/health", get(mock_headless_health_handler));

    let app = Router::new()
        .nest("/v1/maloca", maloca_router)
        .nest("/headless", headless_router);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral test listener");
    let addr = listener.local_addr().expect("retrieve local address");

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve axum ecosystem router");
    });

    EcosystemTestServer {
        base_url: format!("http://{}", addr),
        _temp_dir: temp_dir,
    }
}

/// End-to-end integration test validating full multi-platform ecosystem flows
/// across Web APIs, Desktop/Headless contracts, Memory management, and Mesh P2P sync.
#[tokio::test]
async fn test_full_ecosystem_flow() {
    let server = spawn_ecosystem_test_server().await;
    let client = Client::new();

    // --- Part 1: Web & Headless API Contracts ---
    // 1.1 Web App Registry endpoint
    let registry_res = client
        .get(format!("{}/v1/maloca/registry", server.base_url))
        .send()
        .await
        .expect("query registry endpoint");
    assert_eq!(registry_res.status(), StatusCode::OK);
    let reg_body: MockRegistryResponse = registry_res.json().await.expect("decode registry JSON");
    assert_eq!(reg_body.status, "ok");
    assert_eq!(reg_body.apps.len(), 3);

    // 1.2 Alignment Criteria endpoint
    let align_res = client
        .get(format!("{}/v1/maloca/alignment", server.base_url))
        .send()
        .await
        .expect("query alignment endpoint");
    assert_eq!(align_res.status(), StatusCode::OK);
    let align_body: MockAlignmentResponse = align_res.json().await.expect("decode alignment JSON");
    assert!(align_body.compliant);
    assert_eq!(align_body.goals.len(), 12);

    // 1.3 Headless API Health contract
    let health_res = client
        .get(format!("{}/headless/health", server.base_url))
        .send()
        .await
        .expect("query headless health endpoint");
    assert_eq!(health_res.status(), StatusCode::OK);
    let health_body: MockHeadlessHealthResponse = health_res
        .json()
        .await
        .expect("decode headless health JSON");
    assert_eq!(health_body.service, "xavier-headless-verifier");
    assert_eq!(health_body.mode, "desktop_headless_api");

    // --- Part 2: Desktop Data Node Consent & Quota Management ---
    // 2.1 Query initial consent (default opted out)
    let consent_get = client
        .get(format!("{}/v1/maloca/node/consent", server.base_url))
        .send()
        .await
        .expect("get node consent");
    assert_eq!(consent_get.status(), StatusCode::OK);
    let consent_body_1: DataNodeConsentResponse = consent_get
        .json()
        .await
        .expect("decode consent GET response");
    assert!(!consent_body_1.opt_in);

    // 2.2 Update consent to opt-in with 2048 MB quota
    let update_payload = ConsentUpdateRequest {
        opt_in: Some(true),
        storage_quota_mb: Some(2048),
    };
    let consent_post = client
        .post(format!("{}/v1/maloca/node/consent", server.base_url))
        .json(&update_payload)
        .send()
        .await
        .expect("update node consent");
    assert_eq!(consent_post.status(), StatusCode::OK);
    let consent_body_2: DataNodeConsentResponse = consent_post
        .json()
        .await
        .expect("decode consent POST response");
    assert!(consent_body_2.opt_in);
    assert_eq!(consent_body_2.storage_quota_mb, 2048);

    // --- Part 3: Memory Subsystem Contracts ---
    // 3.1 Cognitive Challenge Session Scanning
    let session_events = vec![
        SessionEvent {
            session_id: "e2e_ecosystem_sess_1".to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some(
                "Sin embargo, esta propuesta contradice la decisión tomada en la reunión anterior."
                    .to_string(),
            ),
            metadata: None,
        },
        SessionEvent {
            session_id: "e2e_ecosystem_sess_1".to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some(
                "Acordamos implementar el almacenamiento local utilizando SQLite.".to_string(),
            ),
            metadata: None,
        },
    ];
    let scanner = SessionScanner::new();
    let candidates = scanner.scan_session_events(&session_events);
    assert!(
        !candidates.is_empty(),
        "Session scanner must detect challenges"
    );

    // 3.2 Semantic Compression Card Generation
    let turn1 = DialogueTurn::new(
        "turn-1",
        "e2e_ecosystem_sess_1",
        "user",
        "We should store system logs in SQLite database file logs.db.",
        0,
    )
    .with_embedding(vec![0.8, 0.2, 0.0, 0.1]);

    let turn2 = DialogueTurn::new(
        "turn-2",
        "e2e_ecosystem_sess_1",
        "assistant",
        "Agreed. Using logs.db for SQLite logging ensures persistence.",
        1,
    )
    .with_embedding(vec![0.79, 0.22, 0.0, 0.09]);

    let turns = vec![turn1, turn2];

    let compressor_config = SemanticCompressorConfig {
        similarity_threshold: 0.75,
        min_cluster_size: 1,
        max_cluster_size: 10,
        target_compression_ratio: 0.70,
        aged_session_hours: 24,
        max_hierarchy_levels: 2,
    };
    let compressor = SemanticCompressor::with_config(compressor_config);
    let result = compressor.compress_session("e2e_ecosystem_sess_1", &turns);
    assert!(!result.cards.is_empty());
    assert!(result.overall_compression_ratio >= 0.0);

    // --- Part 4: Mesh P2P Offline Buffer & Sync Filter ---
    // 4.1 Sync Filter Consent Enforcement
    let node_id = NodeId("xv1-test-node".to_string());
    let peer_id = NodeId("xv1-peer-node".to_string());
    let mut consent_mgr = DataConsentManager::new(node_id);
    consent_mgr.set_consent("workspace_public", ConsentLevel::Full);
    consent_mgr.set_consent("workspace_private", ConsentLevel::None);

    let filter = SyncFilter::new(consent_mgr);
    assert!(filter.is_allowed("workspace_public", &peer_id));
    assert!(!filter.is_allowed("workspace_private", &peer_id));

    // 4.2 SQLite Offline Queue Operations
    let temp_db = server._temp_dir.path().join("offline_queue.db");
    let queue_config = OfflineQueueConfig::default();
    let offline_queue = OfflineQueue::new(&temp_db, queue_config).expect("init offline queue");

    let queued_id = offline_queue
        .enqueue("xv1-peer-node", b"vector_chunk_1".to_vec(), None)
        .expect("queue offline sync event");

    let pending = offline_queue
        .dequeue_retryable(10)
        .expect("list pending events");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, queued_id);

    // --- Part 5: Governance Quadratic Voting Engine ---
    let vote_engine = QuadraticVoteEngine::new(10);
    assert_eq!(vote_engine.default_quorum, 10);

    let effective_votes = calculate_effective_votes(
        16, // 16 credits -> sqrt(16 * karma_weight)
        80, // 80 EigenTrust karma multiplier
        IvnIdentityTier::Verified,
        false, // not sybil flagged
    );
    assert!(effective_votes > 0);
}

#[tokio::test]
async fn test_web_and_desktop_consent_contract() {
    let server = spawn_ecosystem_test_server().await;
    let client = Client::new();

    let res = client
        .get(format!("{}/v1/maloca/node/consent", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body: DataNodeConsentResponse = res.json().await.unwrap();
    assert!(!body.opt_in);
    assert_eq!(body.storage_quota_mb, 1024);
}
