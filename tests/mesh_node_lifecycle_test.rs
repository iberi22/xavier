//! Mesh Node Lifecycle Tests
//!
//! Verifies the lifecycle of start_mesh_node and the active Iroh accept loop.

#![cfg(feature = "mesh")]

use std::sync::Arc;
use xavier::mesh::node::NodeIdentity;
use xavier::mesh::{start_mesh_node_with_transport, IrohTransport};

#[tokio::test]
async fn test_start_mesh_node_lifecycle_and_handshake() {
    // 1. Start Node A using start_mesh_node
    let identity_a = Arc::new(NodeIdentity::generate());
    let store_a: Arc<dyn xavier::memory::MemoryStore> =
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let (transport_a, handle_a) = start_mesh_node_with_transport(
        IrohTransport::new(identity_a.clone(), store_a).localhost_only(),
    )
    .await;

    // 2. Obtain Node A address
    let addr_a = transport_a
        .direct_addr()
        .await
        .expect("Failed to retrieve address for Node A");

    // 3. Start Node B using start_mesh_node
    let identity_b = Arc::new(NodeIdentity::generate());
    let store_b: Arc<dyn xavier::memory::MemoryStore> =
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let (transport_b, handle_b) =
        start_mesh_node_with_transport(IrohTransport::new(identity_b, store_b).localhost_only())
            .await;

    // 4. Perform P2P handshake from Node B to Node A
    let handshake_resp = transport_b
        .handshake_addr(addr_a, None)
        .await
        .expect("Handshake from Node B to Node A failed");

    assert!(
        handshake_resp.accepted,
        "Expected handshake to be accepted by Node A"
    );
    assert_eq!(
        handshake_resp.node_id, identity_a.node_id,
        "Handshake node_id should match Node A's identity"
    );

    // 5. Clean up accept loop background tasks
    handle_a.abort();
    handle_b.abort();
    let _ = handle_a.await;
    let _ = handle_b.await;
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
