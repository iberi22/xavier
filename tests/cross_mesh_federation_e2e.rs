//! E2E Cross-mesh federation and namespace-bounded CrossGrant authorization
//!
//! Tests for asserting Node A and C wallet sync, cross-wallet isolation,
//! cross-grant issuance, and boundary enforcement.

xavier::isolate_test_process!();

use chrono::{Duration, Utc};
use tempfile::tempdir;
use xavier::enterprise::rbac::Permission;
use xavier::mesh::node::NodeIdentity;
use xavier::mesh::private_mesh::{
    derive_wallet_id, encrypt_session_payload, open_private_sync, PrivateMemoryDelta,
    PrivateMeshRegistry, PrivateSyncError, PrivateSyncPayload, WalletNode,
};

#[tokio::test]
async fn test_unauthorized_cross_wallet_sync_rejection() {
    let dir = tempdir().unwrap();
    let mut reg = PrivateMeshRegistry::new(dir.path().join("test_registry.json"));

    let node_a_id = NodeIdentity::generate();
    let wallet_1 = derive_wallet_id(&node_a_id.public_key);
    let node_a = WalletNode {
        node_id: node_a_id.node_id.clone(),
        wallet_id: wallet_1.clone(),
        name: "Node A".to_string(),
        iroh_addr: "".to_string(),
        last_seen: Utc::now(),
    };
    reg.register_wallet_node_with_key(node_a, &wallet_1, &node_a_id.public_key)
        .unwrap();

    let node_c_id = NodeIdentity::generate();
    let wallet_2 = derive_wallet_id(&node_c_id.public_key);
    let node_c = WalletNode {
        node_id: node_c_id.node_id.clone(),
        wallet_id: wallet_2.clone(),
        name: "Node C".to_string(),
        iroh_addr: "".to_string(),
        last_seen: Utc::now(),
    };
    reg.register_wallet_node_with_key(node_c, &wallet_2, &node_c_id.public_key)
        .unwrap();

    // Node A attempts to sync with Node C via standard registry sync target
    let result = reg.sync_target(&wallet_1, &node_c_id.node_id.as_str());
    assert!(result.is_err());

    if let Err(PrivateSyncError::CrossWallet {
        source_wallet,
        target_wallet,
    }) = result
    {
        assert_eq!(source_wallet, wallet_1);
        assert_eq!(target_wallet, wallet_2);
    } else {
        panic!("Expected PrivateSyncError::CrossWallet rejection");
    }
}

#[tokio::test]
async fn test_cross_grant_issuance_and_signing() {
    let dir = tempdir().unwrap();
    let mut reg = PrivateMeshRegistry::new(dir.path().join("test_registry.json"));

    let node_a_id = NodeIdentity::generate();
    let node_c_id = NodeIdentity::generate();

    reg.create_network(
        "net-test".to_string(),
        "Test Network".to_string(),
        node_a_id.node_id.to_string(),
    )
    .unwrap();

    let grant = reg
        .grant_cross(
            "net-test",
            "shared/collab/*".to_string(),
            node_c_id.node_id.to_string(),
            Permission::Read,
            None,
        )
        .unwrap();

    assert_eq!(grant.resource_id, "shared/collab/*");
    assert_eq!(grant.target_node, node_c_id.node_id.as_str());
    assert!(grant.is_active());
}

#[tokio::test]
async fn test_scoped_federation_sync_success() {
    let dir = tempdir().unwrap();
    let mut reg = PrivateMeshRegistry::new(dir.path().join("test_registry.json"));

    let node_a_id = NodeIdentity::generate();
    let node_c_id = NodeIdentity::generate();

    reg.create_network(
        "net-test".to_string(),
        "Test Network".to_string(),
        node_a_id.node_id.to_string(),
    )
    .unwrap();

    reg.grant_cross(
        "net-test",
        "shared/collab/".to_string(), // we can match prefix or just check
        node_c_id.node_id.to_string(),
        Permission::Read,
        None,
    )
    .unwrap();

    // Node A sends memories matching shared/collab/* to Node C
    let payload = PrivateSyncPayload {
        memories: vec![PrivateMemoryDelta {
            path: "shared/collab/doc1".to_string(),
            content: "Secret doc".to_string(),
            metadata: serde_json::json!({}),
            created_at: Utc::now().timestamp(),
        }],
        vectors: vec![],
        snapshots: vec![],
    };

    let target_pubkey: [u8; 32] = node_c_id.public_key.as_slice().try_into().unwrap();
    let encrypted = encrypt_session_payload(&payload, &target_pubkey).unwrap();

    // Node C successfully decrypts and imports them
    let decrypted = open_private_sync(&encrypted, &node_c_id).unwrap();
    assert_eq!(decrypted.memories.len(), 1);

    // Node C's inbound validator (simulated using registry) checks access
    let mut imported = 0;
    for _mem in decrypted.memories {
        if reg.check_permission(&node_c_id.node_id.as_str(), "shared/collab/") {
            imported += 1;
        }
    }
    assert_eq!(imported, 1);
}

#[tokio::test]
async fn test_strict_namespace_isolation_boundary() {
    let dir = tempdir().unwrap();
    let mut reg = PrivateMeshRegistry::new(dir.path().join("test_registry.json"));

    let node_a_id = NodeIdentity::generate();
    let node_c_id = NodeIdentity::generate();

    reg.create_network(
        "net-test".to_string(),
        "Test Network".to_string(),
        node_a_id.node_id.to_string(),
    )
    .unwrap();

    reg.grant_cross(
        "net-test",
        "shared/collab/".to_string(),
        node_c_id.node_id.to_string(),
        Permission::Read,
        None,
    )
    .unwrap();

    // Node A attempts to send a memory with path private/confidential
    let payload = PrivateSyncPayload {
        memories: vec![PrivateMemoryDelta {
            path: "private/confidential/doc1".to_string(),
            content: "Super Secret doc".to_string(),
            metadata: serde_json::json!({}),
            created_at: Utc::now().timestamp(),
        }],
        vectors: vec![],
        snapshots: vec![],
    };

    let target_pubkey: [u8; 32] = node_c_id.public_key.as_slice().try_into().unwrap();
    let encrypted = encrypt_session_payload(&payload, &target_pubkey).unwrap();
    let decrypted = open_private_sync(&encrypted, &node_c_id).unwrap();

    // Node C's inbound validator rejects the record
    let mut imported = 0;
    for mem in decrypted.memories {
        if reg.check_permission(&node_c_id.node_id.as_str(), &mem.path) {
            imported += 1;
        } else if reg.check_permission(&node_c_id.node_id.as_str(), "shared/collab/") {
            if mem.path.starts_with("shared/collab/") {
                imported += 1;
            }
        }
    }
    assert_eq!(
        imported, 0,
        "Namespace isolation boundary must reject unauthorized paths"
    );
}

#[tokio::test]
async fn test_cross_grant_expiration() {
    let dir = tempdir().unwrap();
    let mut reg = PrivateMeshRegistry::new(dir.path().join("test_registry.json"));

    let node_a_id = NodeIdentity::generate();
    let node_c_id = NodeIdentity::generate();

    reg.create_network(
        "net-test".to_string(),
        "Test Network".to_string(),
        node_a_id.node_id.to_string(),
    )
    .unwrap();

    let expired_time = Utc::now() - Duration::minutes(5);

    reg.grant_cross(
        "net-test",
        "shared/collab/".to_string(),
        node_c_id.node_id.to_string(),
        Permission::Read,
        Some(expired_time),
    )
    .unwrap();

    assert!(!reg.check_permission(&node_c_id.node_id.as_str(), "shared/collab/"));
}

#[tokio::test]
async fn test_cross_grant_revocation() {
    let dir = tempdir().unwrap();
    let mut reg = PrivateMeshRegistry::new(dir.path().join("test_registry.json"));

    let node_a_id = NodeIdentity::generate();
    let node_c_id = NodeIdentity::generate();

    reg.create_network(
        "net-test".to_string(),
        "Test Network".to_string(),
        node_a_id.node_id.to_string(),
    )
    .unwrap();

    let grant = reg
        .grant_cross(
            "net-test",
            "shared/collab/".to_string(),
            node_c_id.node_id.to_string(),
            Permission::Read,
            None,
        )
        .unwrap();

    // Immediately after issuance it works
    assert!(reg.check_permission(&node_c_id.node_id.as_str(), "shared/collab/"));

    // Revoke
    reg.revoke_grant("net-test", &grant.id).unwrap();

    // Now it fails
    assert!(!reg.check_permission(&node_c_id.node_id.as_str(), "shared/collab/"));
}
