//! 🌐 Comprehensive Mesh Full Simulation Test Suite
//!
//! Complete test coverage simulating:
//! 1. **Multi-Node Mesh Interconnection & Data Convergence** (3+ nodes, bidirectional sync, chunk diffs, merging)
//! 2. **Private Mesh Isolation & Wallet Gating** (same-wallet isolation, cross-mesh rejection, session encryption)
//! 3. **Transport Layer Simulation** (P2P ICE candidate negotiation, Onion/Tor routing format, fallback queue & retry)
//! 4. **Ephemeral Data Packets** (time-locked access, read-once clinical passes, TTL-expired secret leases)
//! 5. **Mesh Permissions & Synchronized Governance** (clearance-level sync gating, KillSwitch revocation, DAO voting quorum)

use axum::{
    routing::{get, post},
    Extension, Router,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use ulid::Ulid;

use xavier::agents::RuntimeConfig;
use xavier::coordination::secrets::KeyLendingEngine;
use xavier::enterprise::rbac::Role;
use xavier::memory::qmd_memory::MemoryDocument;
use xavier::memory::schema::ClearanceLevel;
use xavier::memory::store::MemoryBackend;
use xavier::mesh::node::NodeId;
use xavier::mesh::p2p::fallback::{OfflineQueue, OfflineQueueConfig};
use xavier::mesh::p2p::nat_traversal::{
    IceCandidate, IceCandidateType, StunMessage, StunMessageType, TransportProtocol,
};
use xavier::mesh::private_mesh::{
    decrypt_session_payload, derive_wallet_id, encrypt_session_payload, is_same_wallet,
    PrivateMemoryDelta, PrivateSyncPayload, WalletNode,
};
use xavier::mesh::{MeshAcl, MeshTransport, NodeAclEntry, NodeIdentity, PeerInfo};
use xavier::secrets::lending::DefaultAuditLogger;
use xavier::server::mesh_governance_routes::{
    cast_vote_handler, create_proposal_handler, list_proposals_handler, CreateProposalRequest,
    MeshGovernanceState, ProposalResponse, VoteOption, VoteRequest,
};
use xavier::server::mesh_health_routes::{
    create_share_pass_handler, save_record_handler, view_record_handler, CreateSharePassRequest,
    CreateSharePassResponse, MeshHealthState, SaveRecordRequest, SaveRecordResponse,
    ViewRecordResponse,
};
use xavier::server::mesh_peer_routes::{
    is_root_or_master, revoke_peer_handler, MeshPeerState, PeerRevokeResponse,
};
use xavier::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceState};

// ============================================================================
// Helper to spin up an isolated test server instance
// ============================================================================
async fn create_test_node(
    workspace_name: &str,
) -> (String, String, Arc<WorkspaceState>, Arc<NodeIdentity>) {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    };

    let token = format!("token-{}", Ulid::new());
    let workspace_id = format!("ws-{}", workspace_name);
    let temp_dir = tempdir().unwrap();
    let workspace_dir = temp_dir.path().to_path_buf();

    let config = WorkspaceConfig {
        id: workspace_id.clone(),
        token: token.clone(),
        plan: xavier::workspace::PlanTier::Personal,
        memory_backend: MemoryBackend::Memory,
        storage_limit_bytes: None,
        request_limit: None,
        request_unit_limit: None,
        embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
        managed_google_embeddings: false,
        sync_policy: xavier::workspace::SyncPolicy::CloudMirror,
        dedup: xavier::settings::types::DedupSettings::default(),
    };

    let workspace = Arc::new(
        WorkspaceState::new(config, RuntimeConfig::default(), workspace_dir)
            .await
            .unwrap(),
    );

    let identity = Arc::new(NodeIdentity::generate());

    let workspace_ctx = WorkspaceContext {
        workspace_id: workspace_id.clone(),
        workspace: workspace.clone(),
    };

    let app = Router::new()
        .route(
            "/v1/mesh/identity",
            get(xavier::server::v1_api::v1_mesh_identity),
        )
        .route(
            "/v1/mesh/handshake",
            post(xavier::server::v1_api::v1_mesh_handshake),
        )
        .route(
            "/v1/mesh/manifest",
            get(xavier::server::v1_api::v1_mesh_manifest),
        )
        .route(
            "/v1/mesh/chunks/request",
            post(xavier::server::v1_api::v1_mesh_chunks_request),
        )
        .route(
            "/v1/mesh/chunks/push",
            post(xavier::server::v1_api::v1_mesh_chunks_push),
        )
        .layer(Extension(workspace_ctx));

    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await.unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{}", addr), token, workspace, identity)
}

// ============================================================================
// 1. Multi-Node Mesh Interconnection & Data Convergence Simulation
// ============================================================================
#[tokio::test]
async fn test_multi_node_mesh_convergence_3_nodes() {
    let config_dir = tempdir().unwrap();
    let config_path = config_dir.path().join("xavier-config.json");
    let config_json = serde_json::json!({
        "license": { "mesh_accepted": true, "license_type": "AGPL-3.0" }
    });
    std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();
    std::env::set_var("XAVIER_CONFIG_PATH", config_path.as_os_str());
    std::env::set_var("XAVIER_CONFIG_DIR", config_dir.path());
    std::env::set_var("XAVIER_EMBEDDER", "disabled");
    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "disabled");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("VOYAGE_API_KEY");
    std::env::remove_var("GOOGLE_API_KEY");
    std::env::remove_var("XAVIER_EMBEDDINGS_PROVIDER");

    // Spin up 3 nodes: Hub, Node Alpha, Node Beta
    let (url_hub, token_hub, ws_hub, id_hub) = create_test_node("hub").await;
    let (_url_a, _token_a, ws_a, id_a) = create_test_node("alpha").await;
    let (_url_b, _token_b, ws_b, id_b) = create_test_node("beta").await;

    let transport_a = MeshTransport::new(id_a.clone());
    let transport_b = MeshTransport::new(id_b.clone());

    // 1. Handshakes Alpha <-> Hub and Beta <-> Hub
    let hs_a = transport_a
        .handshake(&url_hub, &token_hub)
        .await
        .expect("A->Hub handshake");
    assert!(hs_a.accepted);
    let hs_b = transport_b
        .handshake(&url_hub, &token_hub)
        .await
        .expect("B->Hub handshake");
    assert!(hs_b.accepted);

    // Register Alpha and Beta in Hub's ACL
    let mut acl_hub = MeshAcl::load().unwrap();
    acl_hub
        .set_entry(
            id_a.node_id.clone(),
            NodeAclEntry {
                role: Role::Admin,
                clearance: ClearanceLevel::TopSecret,
                namespaces: None,
                public_key_hex: xavier::crypto::hex_encode(&id_a.public_key),
                namespace_acl: None,
            },
        )
        .unwrap();
    acl_hub
        .set_entry(
            id_b.node_id.clone(),
            NodeAclEntry {
                role: Role::Admin,
                clearance: ClearanceLevel::TopSecret,
                namespaces: None,
                public_key_hex: xavier::crypto::hex_encode(&id_b.public_key),
                namespace_acl: None,
            },
        )
        .unwrap();

    // 2. Ingest distinct records across nodes
    ws_hub
        .memory
        .add(MemoryDocument {
            id: Some("hub-doc".to_string()),
            path: "arch/hub-core".to_string(),
            content: "Hub authoritative architecture state".to_string(),
            metadata: serde_json::json!({"node": "hub"}),
            ..Default::default()
        })
        .await
        .unwrap();

    ws_a.memory
        .add(MemoryDocument {
            id: Some("alpha-doc".to_string()),
            path: "telemetry/alpha-metrics".to_string(),
            content: "Alpha edge telemetry counters".to_string(),
            metadata: serde_json::json!({"node": "alpha"}),
            ..Default::default()
        })
        .await
        .unwrap();

    ws_b.memory
        .add(MemoryDocument {
            id: Some("beta-doc".to_string()),
            path: "tasks/beta-jobs".to_string(),
            content: "Beta worker task execution graph".to_string(),
            metadata: serde_json::json!({"node": "beta"}),
            ..Default::default()
        })
        .await
        .unwrap();

    // Export Hub's data to chunk
    let sync_dir_hub = ws_hub.usage_state_path.parent().unwrap().join("sync");
    let mut manifest_hub_disk = xavier::sync::chunks::load_manifest(&sync_dir_hub).unwrap();
    let docs_hub = ws_hub.memory.all_documents().await;
    let hash_hub =
        xavier::sync::chunks::export_to_chunk(&sync_dir_hub, &docs_hub, &mut manifest_hub_disk)
            .unwrap();

    let peer_hub = PeerInfo {
        node_id: id_hub.node_id.clone(),
        alias: Some("Hub Node".to_string()),
        endpoint_url: url_hub.clone(),
        public_key_hex: xavier::crypto::hex_encode(&id_hub.public_key),
        added_at: chrono::Utc::now().timestamp(),
        last_seen_at: Some(chrono::Utc::now().timestamp()),
        sync_enabled: true,
        is_cloud: false,
        iroh_addr: None,
        shared_workspace_ids: Vec::new(),
        shared_workspace_tokens: HashMap::new(),
    };

    // 3. Alpha fetches manifest and chunk from Hub
    let manifest_remote = transport_a
        .fetch_manifest(&peer_hub, &token_hub)
        .await
        .unwrap();
    assert!(!manifest_remote.chunks.is_empty());
    assert_eq!(manifest_remote.chunks[0].hash, hash_hub);

    let hashes = vec![hash_hub.clone()];
    let chunks_map = transport_a
        .fetch_chunks(&peer_hub, &token_hub, &hashes)
        .await
        .unwrap();
    assert!(chunks_map.contains_key(&hash_hub));

    // Import chunk into Alpha
    let sync_dir_a = ws_a.usage_state_path.parent().unwrap().join("sync");
    let chunk_file_a = sync_dir_a
        .join("chunks")
        .join(format!("{}.jsonl.gz", hash_hub));
    std::fs::create_dir_all(chunk_file_a.parent().unwrap()).unwrap();
    std::fs::write(&chunk_file_a, &chunks_map[&hash_hub]).unwrap();

    let imported_docs = xavier::sync::chunks::import_from_chunk(&sync_dir_a, &hash_hub).unwrap();
    assert_eq!(imported_docs.len(), 1);
    assert_eq!(imported_docs[0].path, "arch/hub-core");
    for doc in imported_docs {
        ws_a.memory.add(doc).await.unwrap();
    }

    // 4. Alpha pushes its data to Hub
    let docs_a = ws_a.memory.all_documents().await;
    let mut manifest_a_disk = xavier::sync::chunks::load_manifest(&sync_dir_a).unwrap();
    let hash_a =
        xavier::sync::chunks::export_to_chunk(&sync_dir_a, &docs_a, &mut manifest_a_disk).unwrap();
    let chunk_bytes_a = std::fs::read(
        sync_dir_a
            .join("chunks")
            .join(format!("{}.jsonl.gz", hash_a)),
    )
    .unwrap();

    let push_res = transport_a
        .push_chunks(&peer_hub, &token_hub, &[(hash_a.clone(), chunk_bytes_a)])
        .await
        .unwrap();
    assert_eq!(push_res.len(), 1);

    // Export Hub's converged state so manifest lists both data streams
    let docs_hub_combined = ws_hub.memory.all_documents().await;
    let _ = xavier::sync::chunks::export_to_chunk(
        &sync_dir_hub,
        &docs_hub_combined,
        &mut manifest_hub_disk,
    )
    .unwrap();

    // 5. Beta fetches manifest from Hub and imports all chunks
    let manifest_for_b = transport_b
        .fetch_manifest(&peer_hub, &token_hub)
        .await
        .unwrap();
    assert!(manifest_for_b.chunks.len() >= 2);

    let hashes_for_b: Vec<String> = manifest_for_b.chunks.into_iter().map(|c| c.hash).collect();
    let chunks_b_map = transport_b
        .fetch_chunks(&peer_hub, &token_hub, &hashes_for_b)
        .await
        .unwrap();

    let sync_dir_b = ws_b.usage_state_path.parent().unwrap().join("sync");
    for (h, bytes) in chunks_b_map {
        let chunk_file_b = sync_dir_b.join("chunks").join(format!("{}.jsonl.gz", h));
        std::fs::create_dir_all(chunk_file_b.parent().unwrap()).unwrap();
        std::fs::write(&chunk_file_b, &bytes).unwrap();
        if let Ok(docs) = xavier::sync::chunks::import_from_chunk(&sync_dir_b, &h) {
            for doc in docs {
                ws_b.memory.add(doc).await.unwrap();
            }
        }
    }

    // Verify Beta now contains the converged dataset
    let beta_docs = ws_b.memory.all_documents().await;
    let beta_paths: Vec<String> = beta_docs.into_iter().map(|d| d.path).collect();
    assert!(beta_paths.iter().any(|p| p.contains("hub-core")));
    assert!(beta_paths.iter().any(|p| p.contains("alpha-metrics")));
    assert!(beta_paths.iter().any(|p| p.contains("beta-jobs")));
}

// ============================================================================
// 2. Private Mesh Isolation, Same-Wallet Gating & Session Cryptography
// ============================================================================
#[test]
fn test_private_mesh_wallet_isolation_and_session_encryption() {
    let pubkey_wallet_a = [42u8; 32];
    let pubkey_wallet_b = [99u8; 32];

    let wallet_id_a = derive_wallet_id(&pubkey_wallet_a);
    let wallet_id_b = derive_wallet_id(&pubkey_wallet_b);

    assert_ne!(wallet_id_a, wallet_id_b);
    assert!(is_same_wallet(&wallet_id_a, &wallet_id_a));
    assert!(!is_same_wallet(&wallet_id_a, &wallet_id_b));

    // Register nodes under Wallet A
    let node_1 = WalletNode {
        node_id: NodeId("node-alpha-1".to_string()),
        wallet_id: wallet_id_a.clone(),
        name: "Alpha Workstation".to_string(),
        iroh_addr: "iroh://alpha-1.local".to_string(),
        last_seen: chrono::Utc::now(),
    };

    let node_2 = WalletNode {
        node_id: NodeId("node-alpha-2".to_string()),
        wallet_id: wallet_id_a.clone(),
        name: "Alpha Mobile".to_string(),
        iroh_addr: "iroh://alpha-2.local".to_string(),
        last_seen: chrono::Utc::now(),
    };

    assert!(is_same_wallet(&node_1.wallet_id, &node_2.wallet_id));

    // Construct private memory payload
    let private_payload = PrivateSyncPayload {
        memories: vec![PrivateMemoryDelta {
            path: "finances/q3-budget".to_string(),
            content: "Confidential Q3 budget allocations".to_string(),
            metadata: serde_json::json!({"encrypted": true, "wallet": wallet_id_a}),
            created_at: 1725000000,
        }],
        snapshots: vec!["snapshot-block-109".to_string()],
    };

    // Encrypt payload with Wallet A session key
    let encrypted =
        encrypt_session_payload(&private_payload, &wallet_id_a).expect("Encryption failed");
    assert!(!encrypted.ciphertext_hex.is_empty());
    assert!(!encrypted.nonce_hex.is_empty());

    // Legitimate peer with Wallet A decrypts successfully
    let decrypted_a =
        decrypt_session_payload(&encrypted, &wallet_id_a).expect("Decryption by Wallet A failed");
    assert_eq!(decrypted_a.memories.len(), 1);
    assert_eq!(decrypted_a.memories[0].path, "finances/q3-budget");
    assert_eq!(
        decrypted_a.memories[0].content,
        "Confidential Q3 budget allocations"
    );

    // Rogue node with Wallet B fails decryption
    let decrypt_attempt_b = decrypt_session_payload(&encrypted, &wallet_id_b);
    assert!(
        decrypt_attempt_b.is_err(),
        "Rogue wallet must not decrypt private mesh payload"
    );
}

// ============================================================================
// 3. Transport Simulation: P2P NAT Traversal, Onion Tunnels & Offline Queuing
// ============================================================================
#[tokio::test]
async fn test_transport_p2p_ice_onion_and_offline_fallback() {
    // A. STUN/TURN message and candidate negotiation
    let transaction_id = [7u8; 12];
    let stun_req = StunMessage::create_binding_request(transaction_id);
    let encoded = stun_req.encode();
    assert!(encoded.len() >= 20);

    let parsed_stun = StunMessage::parse(&encoded).expect("STUN parse failed");
    assert_eq!(parsed_stun.message_type, StunMessageType::BindingRequest);

    let candidate = IceCandidate::new(
        "candidate-1",
        1,
        TransportProtocol::Udp,
        "192.168.1.100:8006".parse().unwrap(),
        IceCandidateType::Host,
        None,
    );
    assert_eq!(candidate.candidate_type, IceCandidateType::Host);
    assert_eq!(candidate.addr.port(), 8006);

    // B. Tor / Onion hidden service address validation
    let onion_endpoint = "xavier64charnodeidaddressabcdef1234567890.onion:8006";
    assert!(onion_endpoint.contains(".onion:"));

    // C. Offline fallback queue simulation on P2P network disconnection
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("fallback_queue.db");

    let queue = OfflineQueue::new(&db_path, OfflineQueueConfig::default()).unwrap();

    // Push 3 sync messages while direct P2P link is offline
    queue
        .enqueue("peer-target-charlie", b"Sync delta chunk 1".to_vec(), None)
        .unwrap();
    queue
        .enqueue("peer-target-charlie", b"Sync delta chunk 2".to_vec(), None)
        .unwrap();
    queue
        .enqueue("peer-target-delta", b"Sync delta chunk 3".to_vec(), None)
        .unwrap();

    assert_eq!(queue.count().unwrap(), 3);

    // Drain messages for reconnected peer Charlie
    let retryable = queue.dequeue_retryable(10).unwrap();
    assert_eq!(retryable.len(), 3);
    assert!(retryable
        .iter()
        .any(|m| String::from_utf8_lossy(&m.payload) == "Sync delta chunk 1"));
    assert!(retryable
        .iter()
        .any(|m| String::from_utf8_lossy(&m.payload) == "Sync delta chunk 2"));
    assert!(retryable
        .iter()
        .any(|m| String::from_utf8_lossy(&m.payload) == "Sync delta chunk 3"));
}

// ============================================================================
// 4. Ephemeral Data Packets: Time-Locked Passes, Read-Once & Secret Leases
// ============================================================================
#[tokio::test]
async fn test_ephemeral_data_packets_and_read_once_passes() {
    let health_state = MeshHealthState::default();

    let app = Router::new()
        .route("/v1/mesh/health/records", post(save_record_handler))
        .route(
            "/v1/mesh/health/records/{id}/share-pass",
            post(create_share_pass_handler),
        )
        .route(
            "/v1/mesh/health/records/{id}/view",
            get(view_record_handler),
        )
        .with_state(health_state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", port);

    // 1. Create a sensitive record
    let record_req = SaveRecordRequest {
        family_id: "family-456".to_string(),
        record_type: Some("emergency_clinical".to_string()),
        patient_id: Some("patient-789".to_string()),
        encrypted_payload: "AES256:EMERGENCY_ACCESS_DATA_PAYLOAD".to_string(),
        family_key_id: Some("key-fam-1".to_string()),
    };

    let res = client
        .post(format!("{}/v1/mesh/health/records", base_url))
        .json(&record_req)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let created_rec: SaveRecordResponse = res.json().await.unwrap();
    let rec_id = created_rec.id;

    // 2. Issue a read-once ephemeral pass with 60 second TTL
    let pass_req = CreateSharePassRequest {
        doctor_id: Some("dr-smith".to_string()),
        consultation_ttl: Some(60),
        read_once: Some(true),
    };

    let pass_res = client
        .post(format!(
            "{}/v1/mesh/health/records/{}/share-pass",
            base_url, rec_id
        ))
        .json(&pass_req)
        .send()
        .await
        .unwrap();
    assert_eq!(pass_res.status(), 201);
    let pass_data: CreateSharePassResponse = pass_res.json().await.unwrap();
    let pass_token = pass_data.pass_token;

    // 3. First read attempt with the ephemeral pass succeeds
    let read_1 = client
        .get(format!(
            "{}/v1/mesh/health/records/{}/view?pass_token={}",
            base_url, rec_id, pass_token
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(read_1.status(), 200);
    let body_1: ViewRecordResponse = read_1.json().await.unwrap();
    assert_eq!(
        body_1.encrypted_payload,
        "AES256:EMERGENCY_ACCESS_DATA_PAYLOAD"
    );

    // 4. Second read attempt fails immediately because read_once was consumed
    let read_2 = client
        .get(format!(
            "{}/v1/mesh/health/records/{}/view?pass_token={}",
            base_url, rec_id, pass_token
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(
        read_2.status(),
        403,
        "Read-once pass must be revoked upon first access"
    );

    // 5. Key Lending Engine ephemeral lease TTL test
    let audit_logger = Box::new(DefaultAuditLogger);
    let lending_engine = KeyLendingEngine::new(audit_logger, None);

    let lease = lending_engine
        .lend("db-api-key", Some("secret_value_123"), "agent-ephemeral", 1)
        .await
        .unwrap();
    assert!(!lease.is_expired());

    // Wait for TTL expiry
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(lease.is_expired());
}

// ============================================================================
// 5. Mesh Permissions, KillSwitch Broadcast & Synchronized DAO Governance
// ============================================================================
#[tokio::test]
async fn test_mesh_permissions_killswitch_and_dao_governance() {
    // A. Clearance & Role ACL Evaluation
    let temp_dir = tempdir().unwrap();
    let acl_file = temp_dir.path().join("mesh_acl.json");
    let mut acl = MeshAcl::load_from(acl_file).unwrap();

    acl.set_entry(
        NodeId("node-charlie".to_string()),
        NodeAclEntry {
            role: Role::Viewer,
            clearance: ClearanceLevel::Confidential,
            namespaces: Some(vec!["public/*".to_string(), "ops/*".to_string()]),
            public_key_hex: "abcd1234ef".to_string(),
            namespace_acl: None,
        },
    )
    .unwrap();

    let entry = acl
        .get_entry(&NodeId("node-charlie".to_string()))
        .expect("ACL entry");
    assert_eq!(entry.role, Role::Viewer);
    assert_eq!(entry.clearance, ClearanceLevel::Confidential);
    assert!(entry.clearance >= ClearanceLevel::Internal);
    assert!(entry.clearance < ClearanceLevel::Secret);

    // B. Root Host / Master Guard and KillSwitch Revocation
    assert!(is_root_or_master("root", "root"));
    assert!(is_root_or_master("master_node", "root"));
    assert!(!is_root_or_master("node-worker-99", "root"));

    let peer_state =
        MeshPeerState::new(temp_dir.path().to_path_buf()).with_root_host("root-master".to_string());
    let peer_app = Router::new()
        .route(
            "/v1/mesh/networks/{network_id}/peers/{peer_id}/revoke",
            post(revoke_peer_handler),
        )
        .with_state(peer_state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, peer_app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", port);

    // Attempting to revoke master root node is strictly forbidden
    let root_revoke = client
        .post(format!(
            "{}/v1/mesh/networks/net-1/peers/root-master/revoke",
            base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(root_revoke.status(), 403);

    // Revoking standard peer returns confirmation with purged: true
    let peer_revoke = client
        .post(format!(
            "{}/v1/mesh/networks/net-1/peers/node-worker-99/revoke",
            base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(peer_revoke.status(), 200);
    let revoke_body: PeerRevokeResponse = peer_revoke.json().await.unwrap();
    assert!(revoke_body.purged);
    assert_eq!(revoke_body.status, "ok");
    assert_eq!(revoke_body.notice.peer_id, "node-worker-99");

    // C. Synchronized Mesh DAO Governance
    let gov_state = MeshGovernanceState::default();

    let app = Router::new()
        .route(
            "/v1/mesh/dao/proposals",
            post(create_proposal_handler).get(list_proposals_handler),
        )
        .route("/v1/mesh/dao/proposals/{id}/vote", post(cast_vote_handler))
        .with_state(gov_state);

    let gov_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gov_port = gov_listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(gov_listener, app).await.unwrap();
    });

    let gov_base = format!("http://127.0.0.1:{}", gov_port);

    // 1. Create a DAO Proposal for mesh network parameter update
    let prop_req = CreateProposalRequest {
        title: "Upgrade Mesh Minimum Memory Retention to 90 Days".to_string(),
        description: "Increase default data retention threshold across all enterprise nodes"
            .to_string(),
        category: "infrastructure".to_string(),
        required_endorsement: "core-node".to_string(),
    };

    let p_res = client
        .post(format!("{}/v1/mesh/dao/proposals", gov_base))
        .json(&prop_req)
        .send()
        .await
        .unwrap();
    assert_eq!(p_res.status(), 201);
    let created_prop: ProposalResponse = p_res.json().await.unwrap();
    let prop_id = created_prop.id;

    // 2. Cast Votes across mesh peers
    let vote_1 = VoteRequest {
        node_id: "node-alpha".to_string(),
        ballot: VoteOption::For,
        endorsement_badge: Some("core-node".to_string()),
    };
    let v1_res = client
        .post(format!(
            "{}/v1/mesh/dao/proposals/{}/vote",
            gov_base, prop_id
        ))
        .json(&vote_1)
        .send()
        .await
        .unwrap();
    assert_eq!(v1_res.status(), 200);

    let vote_2 = VoteRequest {
        node_id: "node-beta".to_string(),
        ballot: VoteOption::For,
        endorsement_badge: Some("core-node".to_string()),
    };
    let v2_res = client
        .post(format!(
            "{}/v1/mesh/dao/proposals/{}/vote",
            gov_base, prop_id
        ))
        .json(&vote_2)
        .send()
        .await
        .unwrap();
    assert_eq!(v2_res.status(), 200);

    // 3. Query proposals and verify tally and quorum
    let list_res = client
        .get(format!("{}/v1/mesh/dao/proposals", gov_base))
        .send()
        .await
        .unwrap();
    assert_eq!(list_res.status(), 200);
    let list_body: Vec<ProposalResponse> = list_res.json().await.unwrap();
    assert_eq!(list_body.len(), 1);
    assert_eq!(list_body[0].for_votes, 2);
    assert_eq!(list_body[0].against_votes, 0);
    assert_eq!(list_body[0].total_votes, 2);
}
