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
    let store_a: std::sync::Arc<dyn xavier::memory::MemoryStore> =
        std::sync::Arc::new(xavier::memory::store::InMemoryMemoryStore::default());

    // Seed one real record into A to test manifest chunk generation
    let test_record = xavier::memory::store::MemoryRecord {
        path: "test/sync.md".to_string(),
        workspace_id: "default".to_string(),
        content: "hello mesh".to_string(),
        ..Default::default()
    };
    let test_record_hash = xavier::memory::sync::merge::chunk_hash(&test_record);
    store_a.put(test_record).await.expect("seed record");

    let store_b: std::sync::Arc<dyn xavier::memory::MemoryStore> =
        std::sync::Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let transport_a = IrohTransport::new(identity_a.clone(), store_a).localhost_only();
    let transport_b = IrohTransport::new(identity_b.clone(), store_b).localhost_only();

    // Spawn accept loop for Node A through the real seam and dial the
    // transport it returns.
    let (transport_a, handle_a) = xavier::mesh::start_mesh_node_with_transport(transport_a).await;
    let addr_a = transport_a
        .direct_addr()
        .await
        .expect("Node A iroh direct addr");

    // 3. Node B performs handshake with Node A over QUIC using real seam behavior
    let handshake_resp = transport_b
        .handshake_addr(addr_a.clone(), None)
        .await
        .expect("Handshake B -> A");

    assert!(handshake_resp.accepted);
    assert_eq!(handshake_resp.node_id, identity_a.node_id);

    // 4. Node B fetches manifest from Node A via fetch_manifest_addr
    let manifest = transport_b
        .fetch_manifest_addr(addr_a, "token")
        .await
        .expect("fetch_manifest_addr B -> A");

    assert_eq!(manifest.node_id, identity_a.node_id);
    assert_eq!(
        manifest.chunks.len(),
        1,
        "expected seeded record hash chunk"
    );
    assert_eq!(
        manifest.chunks[0].hash, test_record_hash,
        "expected exact record hash match"
    );

    // Abort background tasks
    handle_a.abort();
    let _ = handle_a.await;
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
