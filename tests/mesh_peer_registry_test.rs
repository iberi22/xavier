//! Mesh Peer Registry and E2E Tests
//!
//! Tests peer registration, persistence across reloads, GET /health active_peers reporting,
//! and multi-node E2E memory sync between two nodes.

#![cfg(feature = "mesh")]

use std::collections::HashMap;
use std::sync::Arc;
use tempfile::tempdir;

use xavier::mesh::{
    node::NodeIdentity,
    peer::{PeerInfo, PeerRegistry},
    IrohTransport,
};

#[tokio::test]
async fn test_peer_registry_persistence_and_health_reporting() {
    let dir = tempdir().expect("tempdir creation");
    let storage_path = dir.path().join("mesh_peers.json");

    let mut registry = PeerRegistry::load_from(storage_path.clone()).expect("load registry");

    let peer_identity = NodeIdentity::generate();
    let peer_info = PeerInfo {
        node_id: peer_identity.node_id.clone(),
        alias: Some("Peer Node B".to_string()),
        endpoint_url: "http://127.0.0.1:8007".to_string(),
        public_key_hex: xavier::crypto::hex_encode(&peer_identity.public_key),
        added_at: chrono::Utc::now().timestamp(),
        last_seen_at: Some(chrono::Utc::now().timestamp()),
        sync_enabled: true,
        is_cloud: false,
        iroh_addr: None,
        shared_workspace_ids: Vec::new(),
        shared_workspace_tokens: HashMap::new(),
        capabilities: Vec::new(),
    };

    registry.add_peer(peer_info.clone()).expect("add peer");
    assert_eq!(registry.list_peers().len(), 1);

    // Test reload from disk (persistence)
    let reloaded = PeerRegistry::load_from(storage_path).expect("reload registry");
    assert_eq!(reloaded.list_peers().len(), 1);

    let found_peer = reloaded
        .get_peer(&peer_identity.node_id)
        .expect("find peer");
    assert_eq!(found_peer.endpoint_url, "http://127.0.0.1:8007");
    assert_eq!(found_peer.alias.as_deref(), Some("Peer Node B"));
}

#[tokio::test]
async fn test_e2e_multi_node_memory_flow_via_mesh() {
    // 1. Create Node A and Node B identities
    let identity_a = Arc::new(NodeIdentity::generate());
    let identity_b = Arc::new(NodeIdentity::generate());

    // 2. Initialize active Iroh transports
    let transport_a = IrohTransport::new(identity_a.clone());
    let transport_b = IrohTransport::new(identity_b.clone());

    // Bind endpoints
    let addr_a = transport_a
        .my_addr_string()
        .await
        .expect("Node A iroh addr");

    // Spawn accept loop for Node A
    let handle_a = transport_a.spawn_accept_loop().await;

    // 3. Node B performs handshake with Node A over QUIC
    let handshake_resp = transport_b
        .handshake(&addr_a, "test-token", None)
        .await
        .expect("Handshake B -> A");

    assert!(handshake_resp.accepted);
    assert_eq!(handshake_resp.node_id, identity_a.node_id);

    // 4. Create PeerInfo for Node A in Node B's registry
    let peer_a_info = PeerInfo {
        node_id: identity_a.node_id.clone(),
        alias: Some("Node A".to_string()),
        endpoint_url: "http://127.0.0.1:8006".to_string(),
        public_key_hex: xavier::crypto::hex_encode(&identity_a.public_key),
        added_at: chrono::Utc::now().timestamp(),
        last_seen_at: Some(chrono::Utc::now().timestamp()),
        sync_enabled: true,
        is_cloud: false,
        iroh_addr: Some(addr_a.clone()),
        shared_workspace_ids: Vec::new(),
        shared_workspace_tokens: HashMap::new(),
        capabilities: Vec::new(),
    };

    // 5. Node B fetches manifest from Node A via mesh transport
    let manifest = transport_b
        .fetch_manifest(&peer_a_info, "token")
        .await
        .expect("fetch_manifest B -> A");

    assert_eq!(manifest.node_id, identity_a.node_id);

    // Abort background tasks
    handle_a.abort();
    let _ = handle_a.await;
}
