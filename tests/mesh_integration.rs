//! Mesh Integration Tests
//!
//! Tests the full handshake and sync flow between two Xavier nodes.

use axum::{
    routing::{get, post},
    Extension, Router,
};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::net::TcpListener;
use ulid::Ulid;
use xavier::agents::RuntimeConfig;
use xavier::enterprise::rbac::Role;
use xavier::memory::qmd_memory::MemoryDocument;
use xavier::memory::schema::ClearanceLevel;
use xavier::memory::store::MemoryBackend;
use xavier::mesh::{MeshTransport, NodeIdentity, PeerInfo};
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
            let transport = xavier::mesh::init_active_transport(identity);
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

            let res_ok =
                SyncTransport::for_peer(&peer_with_iroh, Arc::new(NodeIdentity::generate()));
            assert!(res_ok.is_ok());

            let res_err =
                SyncTransport::for_peer(&peer_no_iroh, Arc::new(NodeIdentity::generate()));
            assert!(res_err.is_ok());
        });
    }

    #[test]
    fn test_iroh_transport_signed_sync_request() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let identity = Arc::new(NodeIdentity::generate());
            let transport = xavier::mesh::init_active_transport(identity.clone());
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
            let transport = xavier::mesh::init_active_transport(identity);
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
            let transport = xavier::mesh::init_active_transport(identity);

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
