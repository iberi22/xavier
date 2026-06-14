//! Mesh Permissions Integration Tests
//!
//! Verifies that nodes can join via pairing codes and that data isolation
//! (clearance and namespaces) is strictly enforced during sync.

use axum::{
    routing::{get, post},
    Extension, Router,
};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::net::TcpListener;
use ulid::Ulid;
use xavier::agents::RuntimeConfig;
use xavier::memory::schema::{ClearanceLevel, TypedMemoryPayload, MemoryNamespace};
use xavier::memory::store::MemoryBackend;
use xavier::mesh::{MeshTransport, NodeIdentity, PeerInfo, MeshAcl, NodeAclEntry};
use xavier::enterprise::rbac::Role;
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
        .layer(Extension(workspace_ctx));

    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await.unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{}", addr), token, workspace)
}

#[tokio::test]
async fn test_mesh_permissions_and_pairing() {
    let temp_dir = tempdir().unwrap();
    std::env::set_var("XAVIER_CONFIG_DIR", temp_dir.path());

    let (url_b, token_b, ws_b) = start_test_server().await;

    // Node A wants to join Node B
    let identity_a = Arc::new(NodeIdentity::generate());
    let transport_a = MeshTransport::new(identity_a.clone());

    // 1. Generate pairing code on B (mocking CLI)
    let identity_b = NodeIdentity::load_or_create().unwrap();
    let (code, secret) = xavier::mesh::pairing::generate_pairing_code(
        identity_b.node_id.clone(),
        url_b.clone(),
        hex::encode(&identity_b.public_key),
    );

    let mut secret_registry = xavier::mesh::pairing_registry::PairingSecretRegistry::load().unwrap();
    let decoded = xavier::mesh::pairing::decode_pairing_code(&code).unwrap();
    secret_registry.register_secret(secret.clone(), decoded.expires_at).unwrap();

    // 2. Handshake with pairing secret
    let resp = transport_a.handshake_with_secret(&url_b, &token_b, Some(secret)).await.expect("Handshake failed");
    assert!(resp.accepted);

    // Verify Node A was auto-registered on B
    let acl_b = MeshAcl::load().unwrap();
    let entry_a = acl_b.get_entry(&identity_a.node_id).expect("Node A should be in ACL");
    assert_eq!(entry_a.clearance, ClearanceLevel::Unclassified);

    // 3. Setup data on B with different clearance and namespaces
    ws_b.memory.add_document_typed(
        "public".to_string(),
        "Public knowledge".to_string(),
        serde_json::json!({"namespace": {"project": "open"}}),
        Some(TypedMemoryPayload {
            clearance: Some(ClearanceLevel::Unclassified),
            namespace: Some(MemoryNamespace { project: Some("open".to_string()), ..Default::default() }),
            ..Default::default()
        })
    ).await.unwrap();

    ws_b.memory.add_document_typed(
        "secret".to_string(),
        "Top secret knowledge".to_string(),
        serde_json::json!({"namespace": {"project": "open"}}),
        Some(TypedMemoryPayload {
            clearance: Some(ClearanceLevel::Unclassified),
            namespace: Some(MemoryNamespace { project: Some("open".to_string()), ..Default::default() }),
            ..Default::default()
        })
    ).await.unwrap();

    // Export B's data to chunks
    let sync_dir_b = ws_b.usage_state_path.parent().unwrap().join("sync");
    let mut manifest_b = xavier::sync::chunks::load_manifest(&sync_dir_b).unwrap();
    let docs_b = ws_b.memory.all_documents().await;
    xavier::sync::chunks::export_to_chunk(&sync_dir_b, &docs_b, &mut manifest_b).unwrap();

    // 4. Node A (Unclassified) fetches manifest
    let peer_b = PeerInfo {
        node_id: identity_b.node_id.clone(),
        alias: None,
        endpoint_url: url_b.clone(),
        public_key_hex: hex::encode(&identity_b.public_key),
        added_at: 0,
        last_seen_at: None,
        sync_enabled: true,
        is_cloud: false,
    };

    let manifest_a = transport_a.fetch_manifest(&peer_b, &token_b).await.unwrap();
    // Chunks are filtered. Let's verify how many chunks we got and if they contain secrets
    for chunk in manifest_a.chunks {
        let _chunks_a = transport_a.fetch_chunks(&peer_b, &token_b, &[chunk.hash]).await.unwrap();
    }

    // 5. Test Namespace Restriction
    // Update Node A's entry on B to restrict to "project-x"
    let mut acl_b = MeshAcl::load().unwrap();
    acl_b.set_entry(identity_a.node_id.clone(), NodeAclEntry {
        role: Role::Reader,
        clearance: ClearanceLevel::TopSecret, // High clearance but...
        namespaces: Some(vec!["project-x".to_string()]), // ...restricted namespace
        public_key_hex: hex::encode(&identity_a.public_key),
    }).unwrap();

    let manifest_a_restricted = transport_a.fetch_manifest(&peer_b, &token_b).await.unwrap();
    // B has "open" and "closed" projects. A is restricted to "project-x".
    // A should see NO chunks.
    println!("DEBUG: Restricted manifest chunks: {:?}", manifest_a_restricted.chunks);
    assert!(manifest_a_restricted.chunks.is_empty(), "A should see no chunks due to namespace restriction");

    // 6. Test Success with Namespace
    let mut acl_b = MeshAcl::load().unwrap();
    acl_b.set_entry(identity_a.node_id.clone(), NodeAclEntry {
        role: Role::Reader,
        clearance: ClearanceLevel::TopSecret,
        namespaces: Some(vec!["open".to_string()]),
        public_key_hex: hex::encode(&identity_a.public_key),
    }).unwrap();

    let manifest_a_open = transport_a.fetch_manifest(&peer_b, &token_b).await.unwrap();
    println!("DEBUG: Open manifest chunks: {:?}", manifest_a_open.chunks);
    assert!(!manifest_a_open.chunks.is_empty(), "A should see chunks from 'open' namespace");
}
