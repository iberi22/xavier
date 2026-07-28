//! E2E — SWAL decentralized login pipeline (F0 → F1 → F2 → F3).
//!
//! Validates end-to-end without network / Stripe / mesh L1:
//! create vault → persist → recover shares → mesh challenge → hybrid pack → Polygon dry-run anchor.

use xavier::crypto::hex_encode;
use xavier::mesh::challenge::{
    create_signed_nonce_challenge, sign_nonce_challenge, verify_nonce_response,
};
use xavier::mesh::namespace::{namespaces_are_isolated, swal_namespace};
use xavier::mesh::node::NodeIdentity;
use xavier::mesh::pro_gate::{evaluate_pro_status, NodeProStatus, ProGateInput};
use xavier::node_identity::{
    HybridPackSignature, NodeBootstrap, NodeStore, NodeStorePaths, OrderMode, OrderedChallenge,
    PublicNodeIdentity,
};
use xavier::polygon_anchor::{
    anchor_node_identity, anchor_sealed_pack, AnchorRegistry, MockAnchorTransport,
};
use std::time::Duration;

#[test]
fn e2e_f0_create_persist_recover_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = NodeStore::new(NodeStorePaths::from_data_dir(dir.path()));

    let created = NodeBootstrap::create(Some("swal-e2e"), "e2e-pin-0001", None).unwrap();
    let pub_before = PublicNodeIdentity::from_keys(&created.keys);
    store.save_vault(&created.vault).unwrap();
    store.save_public_identity(&pub_before).unwrap();

    // Brick rule: 1 share alone cannot reconstruct
    assert!(
        xavier::node_identity::ShamirSplit::combine(&created.shares[..1]).is_err()
            || created.shares.len() >= 3
    );
    let one = [created.shares[0].clone()];
    assert!(xavier::node_identity::ShamirSplit::combine(&one).is_err());

    let challenge = OrderedChallenge::new(OrderMode::Desc, &created.check_codes);
    let response = challenge.expected_response(&created.check_codes);
    let recovered = NodeBootstrap::recover_from_shares(
        &created.shares[..2],
        Some("swal-e2e"),
        &response,
        &challenge,
        "e2e-pin-0002",
        None,
    )
    .unwrap();

    assert_eq!(recovered.keys.node_id, created.keys.node_id);
    assert_eq!(recovered.keys.ed25519_public, created.keys.ed25519_public);
    assert_eq!(
        recovered.keys.ml_dsa_commitment,
        created.keys.ml_dsa_commitment
    );

    store.save_vault(&recovered.vault).unwrap();
    let unlocked = store.unlock("e2e-pin-0002", None).unwrap();
    assert_eq!(unlocked.1.node_id.as_str(), pub_before.node_id);
}

#[test]
fn e2e_f1_mesh_challenge_namespace_pro_gate() {
    let bundle = NodeBootstrap::create(None, "mesh-pin", None).unwrap();
    let identity = NodeIdentity::from_derived(&bundle.keys);

    let challenge = create_signed_nonce_challenge(Some(60));
    let response = sign_nonce_challenge(&identity, &challenge).unwrap();
    let expected_commit = hex_encode(&bundle.keys.ml_dsa_commitment);
    assert_eq!(
        response.ml_dsa_commitment_hex.as_deref(),
        Some(expected_commit.as_str())
    );
    let verified = verify_nonce_response(&response).unwrap();
    assert_eq!(verified, identity.node_id);
    assert!(verify_nonce_response(&response).is_err(), "one-shot replay");

    let a = swal_namespace("worldexams", "inst-aaa").unwrap();
    let b = swal_namespace("worldexams", "inst-bbb").unwrap();
    assert!(namespaces_are_isolated(&a, &b));
    assert!(!namespaces_are_isolated(&a, &a));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let status = evaluate_pro_status(&ProGateInput {
        identity_present: true,
        last_heartbeat_unix: Some(now),
        heartbeat_ttl: Duration::from_secs(300),
        xavier_reachable: true,
    });
    assert_eq!(status, NodeProStatus::Active);
    assert_ne!(
        evaluate_pro_status(&ProGateInput {
            identity_present: false,
            ..Default::default()
        }),
        NodeProStatus::Active
    );
}

#[test]
fn e2e_f2_polygon_anchor_dry_run_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let registry = AnchorRegistry {
        root: dir.path().join("anchors"),
    };
    let bundle = NodeBootstrap::create(None, "anchor-pin", None).unwrap();
    let pub_id = PublicNodeIdentity::from_keys(&bundle.keys);

    let (payload, receipt) = anchor_node_identity(
        &MockAnchorTransport,
        &pub_id.node_id,
        &pub_id.ed25519_public_hex,
        &pub_id.ml_dsa_commitment_hex,
        Some(&registry),
    )
    .unwrap();
    assert!(!payload.content_hash_hex.is_empty());
    assert!(receipt.tx_hash.starts_with("mock:"));
    assert!(receipt.dry_run);
    let loaded = registry.load(&payload.content_hash_hex).unwrap();
    assert_eq!(loaded.content_hash_hex, payload.content_hash_hex);

    let cipher = b"sealed-pack-ciphertext-never-on-chain";
    let meta = r#"{"app":"swal","v":1}"#;
    let (pack_hash, pack_receipt) =
        anchor_sealed_pack(&MockAnchorTransport, cipher, meta, Some(&registry)).unwrap();
    assert_eq!(pack_hash.len(), 64);
    assert!(pack_receipt.dry_run);
    // ciphertext must not appear in receipt JSON
    let raw = std::fs::read_to_string(registry.root.join(format!("{pack_hash}.json"))).unwrap();
    assert!(!raw.contains("sealed-pack-ciphertext"));
}

#[test]
fn e2e_f3_hybrid_pack_sign_verify() {
    let bundle = NodeBootstrap::create(None, "hybrid-pin", None).unwrap();
    let id = NodeIdentity::from_derived(&bundle.keys);
    let cipher = b"pack-bytes-offchain";
    let meta = r#"{"kind":"sealed"}"#;
    let sig = HybridPackSignature::sign_ed25519(&id, cipher, meta).unwrap();
    sig.verify_ed25519(meta).unwrap();
    assert!(sig.is_hybrid_ready());
    let expected = hex_encode(&bundle.keys.ml_dsa_commitment);
    assert_eq!(sig.ml_dsa_commitment_hex.as_deref(), Some(expected.as_str()));
}

#[test]
fn e2e_full_pipeline_create_to_anchor() {
    // Single narrative: create → challenge → hybrid → anchor (dry-run)
    let dir = tempfile::tempdir().unwrap();
    let store = NodeStore::new(NodeStorePaths::from_data_dir(dir.path()));
    let created = NodeBootstrap::create(Some("pipeline"), "pipe-pin", None).unwrap();
    let pub_id = PublicNodeIdentity::from_keys(&created.keys);
    store.save_vault(&created.vault).unwrap();
    store.save_public_identity(&pub_id).unwrap();

    let identity = NodeIdentity::from_derived(&created.keys);
    let ch = create_signed_nonce_challenge(Some(30));
    let resp = sign_nonce_challenge(&identity, &ch).unwrap();
    assert_eq!(verify_nonce_response(&resp).unwrap(), identity.node_id);

    let sig = HybridPackSignature::sign_ed25519(&identity, b"c", "{}").unwrap();
    sig.verify_ed25519("{}").unwrap();

    let reg = AnchorRegistry {
        root: dir.path().join("anchors"),
    };
    let (_p, receipt) = anchor_node_identity(
        &MockAnchorTransport,
        &pub_id.node_id,
        &pub_id.ed25519_public_hex,
        &pub_id.ml_dsa_commitment_hex,
        Some(&reg),
    )
    .unwrap();
    assert!(receipt.dry_run);
    assert!(store.vault_exists());
}
