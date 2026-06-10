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
use xavier::memory::qmd_memory::MemoryDocument;
use xavier::memory::store::MemoryBackend;
use xavier::mesh::{MeshTransport, NodeIdentity, PeerInfo};
use xavier::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceState};

async fn start_test_server() -> (String, String, Arc<WorkspaceState>) {
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
        node_id: resp.node_id,
        alias: None,
        endpoint_url: url_b,
        public_key_hex: resp.public_key_hex,
        added_at: 0,
        last_seen_at: None,
        sync_enabled: true,
    };

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
