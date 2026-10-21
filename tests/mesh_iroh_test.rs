//! Iroh Transport Integration and Accept Loop Tests
//!
//! Exercises Iroh accept loop and P2P QUIC communication between Xavier nodes.

#![cfg(feature = "mesh")]

use std::sync::Arc;
use xavier::mesh::iroh_transport::IrohTransport;
use xavier::mesh::node::NodeIdentity;
use xavier::mesh::private_mesh::{PrivateMemoryDelta, PrivateSyncPayload};

fn test_store() -> std::sync::Arc<dyn xavier::memory::MemoryStore> {
    std::sync::Arc::new(xavier::memory::store::InMemoryMemoryStore::default())
}

#[tokio::test]
async fn test_spawn_accept_loop_returns_join_handle() {
    let identity = Arc::new(NodeIdentity::generate());
    let transport = IrohTransport::new(identity, test_store());

    let handle = transport.spawn_accept_loop().await;
    // The task handle is running in background. Abort it to clean up.
    handle.abort();
    let _ = handle.await;
}

/// Direct (relay-less, localhost) transport that serves only `allowed`.
fn direct_node(
    allowed: &[&NodeIdentity],
) -> (
    Arc<NodeIdentity>,
    Arc<dyn xavier::memory::MemoryStore>,
    IrohTransport,
) {
    let identity = Arc::new(NodeIdentity::generate());
    let store = test_store();
    let transport = IrohTransport::new_direct(
        identity.clone(),
        store.clone(),
        allowed.iter().map(|i| i.node_id.clone()),
    );
    (identity, store, transport)
}

#[tokio::test]
async fn test_iroh_accept_loop_handshake_flow() {
    let identity_b = Arc::new(NodeIdentity::generate());
    let (identity_a, _store_a, transport_a) = direct_node(&[&identity_b]);
    let handle_a = transport_a.spawn_accept_loop().await;
    let addr_a = transport_a.direct_addr().await.expect("addr A");

    let transport_b = IrohTransport::new_direct(
        identity_b,
        test_store(),
        std::iter::once(identity_a.node_id.clone()),
    );
    let resp = transport_b
        .handshake_addr(addr_a, None)
        .await
        .expect("Handshake over Iroh failed");

    assert!(resp.accepted, "Expected handshake to be accepted");
    assert_eq!(resp.node_id, identity_a.node_id);

    handle_a.abort();
    let _ = handle_a.await;
}

#[tokio::test]
async fn test_two_node_sealed_memory_sync_and_unknown_peer_rejected() {
    let identity_a = Arc::new(NodeIdentity::generate());
    let (identity_b, store_b, transport_b) = direct_node(&[&identity_a]);
    let handle_b = transport_b.spawn_accept_loop().await;
    let addr_b = transport_b.direct_addr().await.expect("addr B");

    let transport_a = IrohTransport::new_direct(
        identity_a.clone(),
        test_store(),
        std::iter::once(identity_b.node_id.clone()),
    );
    let payload = PrivateSyncPayload {
        memories: vec![PrivateMemoryDelta {
            path: "notes/mesh-sync".to_string(),
            content: "hello from node A".to_string(),
            metadata: serde_json::json!({}),
            created_at: chrono::Utc::now().timestamp(),
        }],
        vectors: vec![],
        snapshots: vec![],
    };

    let ack = transport_a
        .send_memory_sync(addr_b.clone(), &identity_b.public_key, &payload)
        .await
        .expect("sync from A");
    assert!(ack.accepted, "known sender rejected: {:?}", ack.reason);
    assert_eq!(ack.applied, 1);
    let got = store_b
        .get("mesh-sync", "notes/mesh-sync")
        .await
        .expect("store get")
        .expect("record applied on B");
    assert_eq!(got.content, "hello from node A");

    // Unknown identity (not on B's allowlist): handshake and sync are refused.
    let transport_c = IrohTransport::new_direct(
        Arc::new(NodeIdentity::generate()),
        test_store(),
        std::iter::once(identity_b.node_id.clone()),
    );
    let hs = transport_c
        .handshake_addr(addr_b.clone(), None)
        .await
        .expect("handshake round trip");
    assert!(!hs.accepted, "unknown peer must be rejected");
    let ack_c = transport_c
        .send_memory_sync(addr_b, &identity_b.public_key, &payload)
        .await
        .expect("sync round trip");
    assert!(!ack_c.accepted, "unknown peer sync must be rejected");
    assert_eq!(ack_c.applied, 0);

    handle_b.abort();
    let _ = handle_b.await;
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
