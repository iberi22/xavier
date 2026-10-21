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
use xavier::mesh::data_consent::{ConsentLevel, DataConsentManager};
use xavier::mesh::data_sanitizer::DataSanitizer;
use xavier::mesh::node::NodeId;
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
        protocol: Default::default(),
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

/// Registers a node under the wallet owned by `key`, via the challenge-response flow.
async fn register_wallet_node_live(
    client: &reqwest::Client,
    url: &str,
    key: &NodeIdentity,
    node_id: &str,
    iroh_addr: &str,
) -> reqwest::StatusCode {
    let wallet_id = xavier::mesh::derive_wallet_id(&key.public_key);
    let challenge: xavier::mesh::wallet_ownership::WalletChallenge = client
        .get(format!(
            "{}/v1/f12/private-mesh/challenge?wallet_id={}",
            url, wallet_id
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let proof = xavier::mesh::wallet_ownership::sign_wallet_registration(key, &challenge, node_id);

    client
        .post(format!("{}/v1/f12/private-mesh/nodes", url))
        .json(&serde_json::json!({
            "node_id": node_id,
            "wallet_id": wallet_id,
            "name": node_id,
            "iroh_addr": iroh_addr,
            "proof": proof,
        }))
        .send()
        .await
        .unwrap()
        .status()
}

#[tokio::test]
#[serial]
async fn test_e2e_cross_wallet_rejected_live() {
    let (url, _dir, _state) = start_f12_server().await;
    let client = reqwest::Client::new();
    let key_a = NodeIdentity::generate();
    let key_b = NodeIdentity::generate();
    let wallet_a = xavier::mesh::derive_wallet_id(&key_a.public_key);

    // Register node A under wallet-a, proving ownership of wallet-a's key
    let reg_a = register_wallet_node_live(&client, &url, &key_a, "node-a", "addr-a").await;
    assert_eq!(reg_a, reqwest::StatusCode::CREATED);

    // Register node B under wallet-b, proving ownership of wallet-b's key
    let reg_b = register_wallet_node_live(&client, &url, &key_b, "node-b", "addr-b").await;
    assert_eq!(reg_b, reqwest::StatusCode::CREATED);

    // Attempt sync targeting node B, proving wallet-a's key -> 403 Forbidden
    let challenge: xavier::mesh::wallet_ownership::WalletChallenge = client
        .get(format!(
            "{}/v1/f12/private-mesh/challenge?wallet_id={}",
            url, wallet_a
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let proof =
        xavier::mesh::wallet_ownership::sign_wallet_action(&key_a, &challenge, "sync", "node-b");
    let sync_res = client
        .post(format!("{}/v1/f12/private-mesh/sync", url))
        .json(&serde_json::json!({
            "wallet_id": wallet_a,
            "target_node": "node-b",
            "proof": proof,
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

    // Publishes must carry a proof signed by the publishing node's key.
    let publisher = NodeIdentity::generate();
    let challenge: xavier::mesh::wallet_ownership::WalletChallenge = client
        .get(format!(
            "{}/v1/f12/service-network/challenge?public_key_hex={}",
            url,
            xavier::crypto::hex_encode(&publisher.public_key)
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let raw_payload = "memory_count=user@example.com index_files=/home/user/secret.txt";
    let scope = xavier::mesh::service_network::telemetry_publish_scope(
        &publisher.node_id,
        &ServiceKind::Custom("ops".to_string()),
        raw_payload,
        0,
    );
    let proof =
        xavier::mesh::service_network::sign_telemetry_publish(&publisher, &challenge, &scope);
    let pub_res = client
        .post(format!("{}/v1/f12/service-network/telemetry", url))
        .json(&serde_json::json!({
            "node_id": publisher.node_id.as_str(),
            "payload": raw_payload,
            "proof": proof,
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

#[tokio::test]
#[serial]
async fn test_e2e_data_commons() {
    let (_url, _token, ws) = start_test_server().await;

    // Export record containing a fake secret (sk-...) and fake PII (email, phone number)
    let fake_secret = "sk-fake-not-a-real-key";
    let fake_email = "alice.smith@example.org";
    let fake_phone = "+1-555-019-2834";

    let raw_record = serde_json::json!({
        "metric_name": "data_commons_export",
        "api_key": fake_secret,
        "email": fake_email,
        "phone": fake_phone,
        "payload": "node-diagnostics-public-sample",
        "timestamp": 1700000000
    });

    // Store in workspace memory to demonstrate real export from node memory
    ws.memory
        .add(MemoryDocument {
            id: Some("data-commons-rec-1".to_string()),
            path: "commons/export/sample".to_string(),
            content: "Telemetry diagnostic record for Data Commons".to_string(),
            metadata: raw_record.clone(),
            ..Default::default()
        })
        .await
        .expect("Failed to store export document");

    let all_docs = ws.memory.all_documents().await;
    let doc = all_docs
        .iter()
        .find(|d| d.id.as_deref() == Some("data-commons-rec-1"))
        .expect("Document must exist in memory");
    let record_to_export = &doc.metadata;

    let raw_str = serde_json::to_string(record_to_export).unwrap();
    assert!(raw_str.contains("sk-"));
    assert!(raw_str.contains(fake_email));
    assert!(raw_str.contains(fake_phone));

    // Run through Data Commons export/sanitize path
    let mut sanitizer = DataSanitizer::new();
    // Default rules only: the test must not configure its own protection.
    sanitizer.add_default_rules();

    let sanitized_payload = sanitizer
        .sanitize(record_to_export)
        .expect("Data Commons sanitization must succeed");

    let sanitized_str = serde_json::to_string(&sanitized_payload).unwrap();
    assert!(
        !sanitized_str.contains("sk-"),
        "Sanitized payload must not contain secret key"
    );
    assert!(
        !sanitized_str.contains(fake_email),
        "Sanitized payload must not contain email"
    );
    assert!(
        !sanitized_str.contains(fake_phone),
        "Sanitized payload must not contain phone number"
    );

    // Consent enforcement via DataConsentManager
    let node_id = NodeId("node-commons-exporter".to_string());
    let mut consent_mgr = DataConsentManager::new(node_id);

    // 1. Without consent nothing is shared
    assert_eq!(
        consent_mgr.get_consent("data_commons_export"),
        ConsentLevel::None
    );
    let unconsented_share = consent_mgr
        .sanitize_payload("data_commons_export", &sanitized_payload)
        .expect("Consent check must succeed");
    assert!(
        unconsented_share.is_none(),
        "Without consent nothing is shared"
    );

    // 2. With consent the sanitized payload is shared
    consent_mgr.set_consent("data_commons_export", ConsentLevel::Full);
    assert_eq!(
        consent_mgr.get_consent("data_commons_export"),
        ConsentLevel::Full
    );
    let consented_share = consent_mgr
        .sanitize_payload("data_commons_export", &sanitized_payload)
        .expect("Consent check must succeed");
    assert!(
        consented_share.is_some(),
        "With consent the sanitized payload is shared"
    );

    let shared_payload = consented_share.unwrap();
    let shared_str = serde_json::to_string(&shared_payload).unwrap();
    assert!(
        !shared_str.contains("sk-"),
        "Shared payload must not contain secret key"
    );
    assert!(
        !shared_str.contains(fake_email),
        "Shared payload must not contain email"
    );
    assert!(
        !shared_str.contains(fake_phone),
        "Shared payload must not contain phone number"
    );
    assert!(
        shared_str.contains("node-diagnostics-public-sample"),
        "Shared payload must contain sanitized non-PII data"
    );

    // Verify P2P SyncFilter honors the granted consent
    let peer_id = NodeId("node-peer-consumer".to_string());
    let filter = xavier::mesh::p2p::SyncFilter::new(consent_mgr);
    assert!(filter.is_allowed("data_commons_export", &peer_id));
    assert!(!filter.is_allowed("unconsented_data_type", &peer_id));
}

/// Two in-process nodes: node A's F12 route seals a private sync to node B's
/// registered key and delivers it over the sync transport; node B decrypts it
/// with its own secret and applies it to its store.
#[cfg(feature = "mesh")]
#[tokio::test]
#[serial]
async fn test_e2e_private_sync_two_nodes() {
    use xavier::mesh::iroh_transport::IrohTransport;
    use xavier::sync::transport::SyncTransport;

    let owner = NodeIdentity::generate();
    let wallet = xavier::mesh::derive_wallet_id(&owner.public_key);
    let identity_a = Arc::new(NodeIdentity::generate());
    let identity_b = Arc::new(NodeIdentity::generate());

    // Node B: localhost-only Iroh endpoint that accepts sync from node A.
    let store_b: Arc<dyn xavier::memory::MemoryStore> =
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let transport_b = IrohTransport::new_direct(
        identity_b.clone(),
        store_b.clone(),
        std::iter::once(identity_a.node_id.clone()),
    );
    let accept_b = transport_b.spawn_accept_loop().await;
    let addr_b = serde_json::to_string(&transport_b.direct_addr().await.expect("addr B")).unwrap();

    // Node A: F12 HTTP surface whose private sync goes out over its own Iroh endpoint.
    let dir_a = tempdir().unwrap();
    let transport_a = SyncTransport::P2pIroh(IrohTransport::new_direct(
        identity_a.clone(),
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default()),
        std::iter::once(identity_b.node_id.clone()),
    ));
    let state = F12State::new(dir_a.path().to_path_buf())
        .with_private_sync_transport(Arc::new(transport_a));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = f12_routes::router(state);
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let challenge = |client: reqwest::Client, url: String, wallet: String| async move {
        client
            .get(format!(
                "{}/v1/f12/private-mesh/challenge?wallet_id={}",
                url, wallet
            ))
            .send()
            .await
            .unwrap()
            .json::<xavier::mesh::wallet_ownership::WalletChallenge>()
            .await
            .unwrap()
    };

    // Register node B with its node key and dialable address.
    let node_b = identity_b.node_id.as_str().to_string();
    let reg_challenge = challenge(client.clone(), url.clone(), wallet.clone()).await;
    let reg_proof =
        xavier::mesh::wallet_ownership::sign_wallet_registration(&owner, &reg_challenge, &node_b);
    let reg = client
        .post(format!("{}/v1/f12/private-mesh/nodes", url))
        .json(&serde_json::json!({
            "node_id": node_b,
            "wallet_id": wallet,
            "name": "node-b",
            "iroh_addr": addr_b,
            "public_key_hex": xavier::crypto::hex_encode(&identity_b.public_key),
            "proof": reg_proof,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reg.status(), reqwest::StatusCode::CREATED);

    let sync_challenge = challenge(client.clone(), url.clone(), wallet.clone()).await;
    let sync_proof = xavier::mesh::wallet_ownership::sign_wallet_action(
        &owner,
        &sync_challenge,
        "sync",
        &node_b,
    );
    let resp = client
        .post(format!("{}/v1/f12/private-mesh/sync", url))
        .json(&serde_json::json!({
            "wallet_id": wallet,
            "target_node": node_b,
            "proof": sync_proof,
            "memories": [{
                "path": "notes/private-sync",
                "content": "delivered to node B",
                "metadata": {},
                "created_at": chrono::Utc::now().timestamp(),
            }],
            "snapshots": ["snap-not-applied"],
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.text().await.unwrap();
    assert_eq!(status, reqwest::StatusCode::OK, "sync failed: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["synced_memories"], 1);
    assert_eq!(v["synced_snapshots"], 0, "node B applies no snapshots");
    assert_eq!(v["target_node"], node_b.as_str());

    let applied = store_b
        .get("mesh-sync", "notes/private-sync")
        .await
        .unwrap()
        .expect("memory applied on node B");
    assert_eq!(applied.content, "delivered to node B");
    assert!(
        !dir_a.path().join("sync/memories").exists(),
        "node A must not apply the delta locally"
    );

    server.abort();
    accept_b.abort();
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
