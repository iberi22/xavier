//! Mesh Node Lifecycle Tests
//!
//! Verifies the lifecycle of start_mesh_node and the active Iroh accept loop.

#![cfg(feature = "mesh")]

use std::sync::Arc;
use xavier::mesh::node::NodeIdentity;
use xavier::mesh::start_mesh_node;

#[tokio::test]
async fn test_start_mesh_node_lifecycle_and_handshake() {
    // 1. Start Node A using start_mesh_node
    let identity_a = Arc::new(NodeIdentity::generate());
    let (transport_a, handle_a) = start_mesh_node(identity_a.clone()).await;

    // 2. Obtain Node A address
    let addr_a = transport_a
        .my_addr_string()
        .await
        .expect("Failed to retrieve address for Node A");

    // 3. Start Node B using start_mesh_node
    let identity_b = Arc::new(NodeIdentity::generate());
    let (transport_b, handle_b) = start_mesh_node(identity_b).await;

    // 4. Perform P2P handshake from Node B to Node A
    let handshake_resp = transport_b
        .handshake(&addr_a, "test-token", None)
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
