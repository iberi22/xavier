//! Iroh Transport Integration and Accept Loop Tests
//!
//! Exercises Iroh accept loop and P2P QUIC communication between Xavier nodes.

#![cfg(feature = "mesh")]

use std::sync::Arc;
use xavier::mesh::iroh_transport::IrohTransport;
use xavier::mesh::node::NodeIdentity;

#[tokio::test]
async fn test_spawn_accept_loop_returns_join_handle() {
    let identity = Arc::new(NodeIdentity::generate());
    let transport = IrohTransport::new(identity);

    let handle = transport.spawn_accept_loop().await;
    // The task handle is running in background. Abort it to clean up.
    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn test_iroh_accept_loop_handshake_flow() {
    let identity_a = Arc::new(NodeIdentity::generate());
    let transport_a = IrohTransport::new(identity_a.clone());

    let handle_a = transport_a.spawn_accept_loop().await;
    let addr_a = transport_a
        .my_addr_string()
        .await
        .expect("Failed to get Node A addr");

    let identity_b = Arc::new(NodeIdentity::generate());
    let transport_b = IrohTransport::new(identity_b);

    let resp = transport_b
        .handshake(&addr_a, "test-token", None)
        .await
        .expect("Handshake over Iroh failed");

    assert!(resp.accepted, "Expected handshake to be accepted");
    assert_eq!(
        resp.node_id, identity_a.node_id,
        "Handshake node_id should match Node A"
    );

    handle_a.abort();
    let _ = handle_a.await;
}
