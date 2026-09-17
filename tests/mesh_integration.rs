//! Mesh Integration Tests
//!
//! Tests the full handshake and sync flow between two Xavier nodes.

use axum::{
    routing::{get, post},
    Extension, Router,
};
use serial_test::serial;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::net::TcpListener;
use ulid::Ulid;
use xavier::agents::RuntimeConfig;
use xavier::enterprise::rbac::Role;
use xavier::memory::qmd_memory::MemoryDocument;
use xavier::memory::schema::ClearanceLevel;
use xavier::memory::store::MemoryBackend;
use xavier::mesh::pairing_registry::PairingSecretRegistry;
use xavier::mesh::service_network::{ServiceKind, ServiceRegistry, TelemetrySample};
use xavier::mesh::{MeshTransport, NodeIdentity, PeerInfo};
use xavier::server::f12_routes::{self, F12State};
use xavier::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceState};

async fn start_test_server() -> (String, String, Arc<WorkspaceState>) {
    // Mesh license must be accepted or the handshake handler returns 403.
    let config_dir = tempdir().unwrap();
    let config_path = config_dir.path().join("xavier-config.json");
    let config_json = serde_json::json!({
        "license": { "mesh_accepted": true, "license_type": "AGPL-3.0" }
    });
    std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();
    unsafe {
        std::env::set_var("XAVIER_CONFIG_PATH", config_path.as_os_str());
        std::env::set_var("XAVIER_CONFIG_DIR", config_dir.path());
    }

    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    };

    let token = format!("test-token-{}", Ulid::new());
    let workspace_id = format!("test-ws-{}", Ulid::new());
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
        dedup: Default::default(),
    };

    let workspace = Arc::new(
        WorkspaceState::new(config, RuntimeConfig::default(), workspace_dir)
            .await
            .unwrap(),
    );

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

    (format!("http://{}", addr), token, workspace)
}

#[tokio::test]
async fn test_mesh_handshake_and_sync() {
    let (_url_a, _token_a, ws_a) = start_test_server().await;
    let (url_b, token_b, ws_b) = start_test_server().await;

    let identity_a = Arc::new(NodeIdentity::generate());
    let transport_a = MeshTransport::new(identity_a.clone());

    // 1. Handshake A -> B
    let resp = transport_a
        .handshake(&url_b, &token_b)
        .await
        .expect("Handshake failed");
    assert!(resp.accepted);
    assert_ne!(resp.node_id, identity_a.node_id);

    // 2. Add some data to B
    ws_b.memory
        .add(MemoryDocument {
            id: Some("doc-1".to_string()),
            path: "test/path".to_string(),
            content: "Hello from Node B".to_string(),
            metadata: serde_json::json!({}),
            embedding: vec![],
            ..Default::default()
        })
        .await
        .expect("Failed to add doc to B");

    // 3. Fetch manifest from B
    let peer_b = PeerInfo {
        node_id: resp.node_id.clone(),
        alias: None,
        endpoint_url: url_b,
        public_key_hex: resp.public_key_hex,
        added_at: 0,
        last_seen_at: None,
        sync_enabled: true,
        is_cloud: false,
        iroh_addr: None,
        shared_workspace_ids: Vec::new(),
        shared_workspace_tokens: std::collections::HashMap::new(),
        capabilities: Vec::new(),
    };

    // Since we now enforce NodeID in manifest request, we must make sure Node A is in Node B's ACL
    // In this test, Node B's environment is not strictly controlled like in permissions_test,
    // so we might need to manually set it up if it doesn't auto-register (it only auto-registers with pairing secret).
    // Let's manually add it to the ACL file.
    let mut acl_b = xavier::mesh::MeshAcl::load().unwrap();
    acl_b
        .set_entry(
            identity_a.node_id.clone(),
            xavier::mesh::NodeAclEntry {
                role: Role::Viewer,
                clearance: ClearanceLevel::TopSecret,
                namespaces: None,
                namespace_acl: None,
                public_key_hex: xavier::crypto::hex_encode(&identity_a.public_key),
            },
        )
        .unwrap();

    // Export B's data to chunks so it appears in manifest
    let mut manifest_b =
        xavier::sync::chunks::load_manifest(&ws_b.usage_state_path.parent().unwrap().join("sync"))
            .unwrap();
    let docs_b = ws_b.memory.all_documents().await;
    let hash_b = xavier::sync::chunks::export_to_chunk(
        &ws_b.usage_state_path.parent().unwrap().join("sync"),
        &docs_b,
        &mut manifest_b,
    )
    .expect("Export failed");

    let manifest = transport_a
        .fetch_manifest(&peer_b, &token_b)
        .await
        .expect("Failed to fetch manifest");
    assert!(!manifest.chunks.is_empty());
    assert_eq!(manifest.chunks[0].hash, hash_b);

    // 4. Fetch chunks from B
    let hashes: Vec<String> = manifest.chunks.iter().map(|c| c.hash.clone()).collect();
    let chunks = transport_a
        .fetch_chunks(&peer_b, &token_b, &hashes)
        .await
        .expect("Failed to fetch chunks");
    assert_eq!(chunks.len(), 1);
    assert!(chunks.contains_key(&hash_b));

    // 5. Push data from A to B
    ws_a.memory
        .add(MemoryDocument {
            id: Some("doc-a".to_string()),
            path: "test/path_a".to_string(),
            content: "Hello from Node A".to_string(),
            metadata: serde_json::json!({}),
            ..Default::default()
        })
        .await
        .unwrap();

    let mut manifest_a =
        xavier::sync::chunks::load_manifest(&ws_a.usage_state_path.parent().unwrap().join("sync"))
            .unwrap();
    let docs_a = ws_a.memory.all_documents().await;
    let hash_a = xavier::sync::chunks::export_to_chunk(
        &ws_a.usage_state_path.parent().unwrap().join("sync"),
        &docs_a,
        &mut manifest_a,
    )
    .unwrap();
    let chunk_data_a = std::fs::read(
        ws_a.usage_state_path
            .parent()
            .unwrap()
            .join("sync")
            .join("chunks")
            .join(format!("{}.jsonl.gz", hash_a)),
    )
    .unwrap();

    let pushed = transport_a
        .push_chunks(&peer_b, &token_b, &[(hash_a.clone(), chunk_data_a)])
        .await
        .expect("Push failed");
    assert_eq!(pushed.len(), 1);
    assert_eq!(pushed[0], hash_a);

    // Verify B now has A's document
    let b_docs = ws_b.memory.all_documents().await;
    assert!(b_docs.iter().any(|d| d.content == "Hello from Node A"));
}

#[cfg(feature = "mesh")]
mod iroh_tests {
    use super::*;
    use xavier::mesh::{HeartbeatStatus, NodeId, NodeIdentity, PeerInfo};
    use xavier::sync::SyncTransport;

    #[test]
    fn test_iroh_transport_init_and_my_addr() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let identity = Arc::new(NodeIdentity::generate());
            let transport =
                xavier::mesh::init_active_transport(identity, xavier::mesh::dummy_store());
            let addr = transport.my_addr_string().await;
            assert!(addr.is_ok());
            let addr_str = addr.unwrap();
            assert!(!addr_str.is_empty());
        });
    }

    #[test]
    fn test_iroh_transport_addr_from_peer() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Peer with iroh_addr
            let peer_with_iroh = PeerInfo {
                node_id: NodeId("peer-1".to_string()),
                alias: None,
                endpoint_url: "".to_string(),
                public_key_hex: "aabb".to_string(),
                added_at: 0,
                last_seen_at: None,
                sync_enabled: true,
                is_cloud: false,
                iroh_addr: Some(
                    "abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234".to_string(),
                ),
                shared_workspace_ids: Vec::new(),
                shared_workspace_tokens: std::collections::HashMap::new(),
                capabilities: Vec::new(),
            };

            // Peer without iroh_addr
            let peer_no_iroh = PeerInfo {
                node_id: NodeId("peer-2".to_string()),
                alias: None,
                endpoint_url: "".to_string(),
                public_key_hex: "aabb".to_string(),
                added_at: 0,
                last_seen_at: None,
                sync_enabled: true,
                is_cloud: false,
                iroh_addr: None,
                shared_workspace_ids: Vec::new(),
                shared_workspace_tokens: std::collections::HashMap::new(),
                capabilities: Vec::new(),
            };

            let res_ok = SyncTransport::for_peer(
                &peer_with_iroh,
                Arc::new(NodeIdentity::generate()),
                xavier::mesh::dummy_store(),
            );
            assert!(res_ok.is_ok());

            let res_err = SyncTransport::for_peer(
                &peer_no_iroh,
                Arc::new(NodeIdentity::generate()),
                xavier::mesh::dummy_store(),
            );
            assert!(res_err.is_ok());
        });
    }

    #[test]
    fn test_iroh_transport_signed_sync_request() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let identity = Arc::new(NodeIdentity::generate());
            let transport =
                xavier::mesh::init_active_transport(identity.clone(), xavier::mesh::dummy_store());
            let request =
                transport.signed_sync_request(vec!["hash-1".to_string(), "hash-2".to_string()]);

            assert_eq!(request.requesting_node_id, identity.node_id);
            assert_eq!(
                request.wanted_hashes,
                vec!["hash-1".to_string(), "hash-2".to_string()]
            );
            assert!(!request.nonce.is_empty());
            assert!(!request.signature_hex.is_empty());
        });
    }

    #[test]
    fn test_iroh_transport_idempotency() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let identity = Arc::new(NodeIdentity::generate());
            let transport =
                xavier::mesh::init_active_transport(identity, xavier::mesh::dummy_store());
            let addr1 = transport.my_addr_string().await.unwrap();
            let addr2 = transport.my_addr_string().await.unwrap();
            assert_eq!(addr1, addr2);
        });
    }

    #[test]
    fn test_iroh_transport_mesh_request_serialization() {
        use xavier::mesh::iroh_transport::MeshRequest;
        use xavier::mesh::protocol::MeshSyncRequest;

        let sync_req = MeshSyncRequest {
            requesting_node_id: NodeId("test-node".to_string()),
            wanted_hashes: vec!["hash1".to_string()],
            timestamp: 1234567890,
            nonce: "test-nonce".to_string(),
            signature_hex: "abcde".to_string(),
        };
        let req = MeshRequest::FetchChunks { request: sync_req };
        let serialized = serde_json::to_string(&req).unwrap();
        assert!(serialized.contains("\"op\":\"fetch_chunks\""));

        let deserialized: MeshRequest = serde_json::from_str(&serialized).unwrap();
        match deserialized {
            MeshRequest::FetchChunks { request } => {
                assert_eq!(request.timestamp, 1234567890);
                assert_eq!(request.nonce, "test-nonce");
            }
            _ => panic!("Expected FetchChunks variant"),
        }
    }

    #[test]
    fn test_iroh_transport_connect_fail() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let identity = Arc::new(NodeIdentity::generate());
            let transport =
                xavier::mesh::init_active_transport(identity, xavier::mesh::dummy_store());

            let invalid_key_res =
                xavier::mesh::connect_active_transport(&transport, "not-a-valid-key").await;
            assert!(invalid_key_res.is_err());
            let err_msg = invalid_key_res.unwrap_err().to_string();
            assert!(err_msg.contains("invalid iroh peer addr"));
        });
    }

    #[test]
    fn test_heartbeat_service_with_peer_count() {
        let svc = HeartbeatStatus::new(NodeId("test-node-hb".to_string())).with_peer_count(42);
        let payload = svc.payload();
        assert_eq!(payload.peer_count, 42);
        assert_eq!(payload.node_id.as_str(), "test-node-hb");
    }
}

async fn start_f12_server() -> (String, tempfile::TempDir, F12State) {
    let temp_dir = tempdir().unwrap();
    std::fs::create_dir_all(temp_dir.path().join("mesh")).ok();
    std::fs::create_dir_all(temp_dir.path().join("security")).ok();
    std::fs::create_dir_all(temp_dir.path().join("curation")).ok();

    let state = F12State::new(temp_dir.path().to_path_buf());
    let app = f12_routes::router(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (url, temp_dir, state)
}

#[tokio::test]
#[serial]
async fn test_e2e_cross_wallet_rejected_live() {
    let (url, _dir, _state) = start_f12_server().await;
    let client = reqwest::Client::new();

    // Register node A under wallet-a
    let reg_a = client
        .post(format!("{}/v1/f12/private-mesh/nodes", url))
        .json(&serde_json::json!({
            "node_id": "node-a",
            "wallet_id": "wallet-a",
            "name": "Node A",
            "iroh_addr": "addr-a"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reg_a.status(), reqwest::StatusCode::CREATED);

    // Register node B under wallet-b
    let reg_b = client
        .post(format!("{}/v1/f12/private-mesh/nodes", url))
        .json(&serde_json::json!({
            "node_id": "node-b",
            "wallet_id": "wallet-b",
            "name": "Node B",
            "iroh_addr": "addr-b"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reg_b.status(), reqwest::StatusCode::CREATED);

    // Attempt sync targeting node B using wallet-a -> 403 Forbidden
    let sync_res = client
        .post(format!("{}/v1/f12/private-mesh/sync", url))
        .json(&serde_json::json!({
            "wallet_id": "wallet-a",
            "target_node": "node-b"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(sync_res.status(), reqwest::StatusCode::FORBIDDEN);
    let err_body = sync_res.text().await.unwrap();
    assert!(
        err_body.contains("Cross-wallet sync rejected"),
        "Expected error message to contain 'Cross-wallet sync rejected', got: {}",
        err_body
    );
}

#[tokio::test]
#[serial]
async fn test_e2e_publish_non_internal_rejected_live() {
    // 1. Direct ServiceRegistry behavior check: publish_telemetry_checked rejects non-INTERNAL or PII
    let mut reg = ServiceRegistry::new();
    let node_id = xavier::mesh::node::NodeId("node-pub".to_string());

    let non_internal_sample = TelemetrySample {
        node_id: node_id.clone(),
        kind: ServiceKind::Memory,
        payload: "clean metric".to_string(),
        ts: 1000,
        classification: "PUBLIC".to_string(),
    };
    let err_res = reg.publish_telemetry_checked(non_internal_sample);
    assert!(err_res.is_err());
    let err_msg = err_res.unwrap_err().to_string();
    assert!(err_msg.contains("must be INTERNAL"));
    assert!(reg.consume_telemetry(0).is_empty());

    let pii_sample = TelemetrySample {
        node_id: node_id.clone(),
        kind: ServiceKind::Memory,
        payload: "user contact alice@example.com".to_string(),
        ts: 1000,
        classification: "INTERNAL".to_string(),
    };
    let pii_err = reg.publish_telemetry_checked(pii_sample);
    assert!(pii_err.is_err());
    assert!(reg.consume_telemetry(0).is_empty());

    // 2. HTTP endpoint check: POST /v1/f12/service-network/telemetry scrubs PII and forces INTERNAL
    let (url, _dir, _state) = start_f12_server().await;
    let client = reqwest::Client::new();

    let pub_res = client
        .post(format!("{}/v1/f12/service-network/telemetry", url))
        .json(&serde_json::json!({
            "node_id": "node-pub",
            "payload": "user user@example.com path /home/user/secret.txt"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(pub_res.status(), reqwest::StatusCode::OK);
    let pub_json: serde_json::Value = pub_res.json().await.unwrap();
    assert_eq!(pub_json["classification"], "INTERNAL");
    let payload_str = pub_json["payload"].as_str().unwrap();
    assert!(!payload_str.contains("user@example.com"));
    assert!(!payload_str.contains("/home/user/secret.txt"));
    assert!(payload_str.contains("[EMAIL]"));
    assert!(payload_str.contains("[PATH]"));

    // Verify GET consume returns no raw PII
    let get_res = client
        .get(format!("{}/v1/f12/service-network/telemetry?since=0", url))
        .send()
        .await
        .unwrap();
    assert_eq!(get_res.status(), reqwest::StatusCode::OK);
    let samples: Vec<serde_json::Value> = get_res.json().await.unwrap();
    assert_eq!(samples.len(), 1);
    let consumed_payload = samples[0]["payload"].as_str().unwrap();
    assert!(!consumed_payload.contains("user@example.com"));
}

#[tokio::test]
#[serial]
async fn test_e2e_join_roundtrip_live() {
    let (url, dir, _state) = start_f12_server().await;
    let client = reqwest::Client::new();

    // 1. Create network
    let create_res = client
        .post(format!("{}/v1/f12/networks", url))
        .json(&serde_json::json!({
            "id": "net-e2e-1",
            "name": "E2E Mesh Network",
            "owner_node": "owner-node"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_res.status(), reqwest::StatusCode::CREATED);

    // 2. Register secret in PairingSecretRegistry
    let secret = "secret-e2e-valid-456".to_string();
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let mut pairing_reg =
        PairingSecretRegistry::load_from(dir.path().join("mesh/pairing-secrets.json")).unwrap();
    pairing_reg
        .register_secret(secret.clone(), now_secs + 3600)
        .unwrap();

    // 3. POST join network with secret
    let join_res = client
        .post(format!("{}/v1/f12/networks/net-e2e-1/join", url))
        .json(&serde_json::json!({
            "node_id": "joining-node",
            "pairing_secret": secret,
            "visibility": "private"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(join_res.status(), reqwest::StatusCode::OK);
    let net_json: serde_json::Value = join_res.json().await.unwrap();
    assert_eq!(net_json["id"], "net-e2e-1");
    let members: Vec<String> = serde_json::from_value(net_json["members"].clone()).unwrap();
    assert!(members.contains(&"joining-node".to_string()));

    // 4. GET ?visibility= filter listing
    let list_priv = client
        .get(format!("{}/v1/f12/networks?visibility=private", url))
        .send()
        .await
        .unwrap();
    assert_eq!(list_priv.status(), reqwest::StatusCode::OK);
    let priv_nets: Vec<serde_json::Value> = list_priv.json().await.unwrap();
    assert_eq!(priv_nets.len(), 1);
    assert_eq!(priv_nets[0]["id"], "net-e2e-1");

    let list_pub = client
        .get(format!("{}/v1/f12/networks?visibility=public", url))
        .send()
        .await
        .unwrap();
    assert_eq!(list_pub.status(), reqwest::StatusCode::OK);
    let pub_nets: Vec<serde_json::Value> = list_pub.json().await.unwrap();
    assert!(pub_nets.is_empty());
}
