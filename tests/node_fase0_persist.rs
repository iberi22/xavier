//! Fase 0 — vault persist + recover identity under a temp data dir
//! (mirrors `xavier node create|recover` without interactive CLI).

use xavier::crypto::hex_encode;
use xavier::node_identity::{
    NodeBootstrap, NodeStore, NodeStorePaths, OrderMode, OrderedChallenge, PublicNodeIdentity,
    ShamirShare,
};

#[test]
fn create_persist_load_unlock_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let store = NodeStore::new(NodeStorePaths::from_data_dir(dir.path()));
    let bundle = NodeBootstrap::create(None, "445566", None).unwrap();
    let pub_id = PublicNodeIdentity::from_keys(&bundle.keys);

    store.save_vault(&bundle.vault).unwrap();
    store.save_public_identity(&pub_id).unwrap();

    let loaded = store.load_vault().unwrap();
    let opened = loaded.unlock("445566", None).unwrap();
    assert_eq!(opened.entropy.len(), 32);

    let (_o, keys, _codes) = store.unlock("445566", None).unwrap();
    assert_eq!(keys.node_id.as_str(), pub_id.node_id);
    assert_eq!(keys.ed25519_public, bundle.keys.ed25519_public);
}

#[test]
fn recover_from_shares_file_same_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = NodeStore::new(NodeStorePaths::from_data_dir(dir.path()));
    let original = NodeBootstrap::create(Some("swal"), "111111", None).unwrap();
    let before = PublicNodeIdentity::from_keys(&original.keys);

    // Same JSON shape as `xavier node create --shares-out`
    let shares_json = serde_json::json!({
        "version": 1,
        "shares": original.shares.iter().take(2).map(|s| serde_json::json!({
            "x": s.x,
            "ys_hex": hex_encode(&s.ys),
        })).collect::<Vec<_>>(),
    });
    let shares_path = dir.path().join("shares.json");
    std::fs::write(&shares_path, serde_json::to_string_pretty(&shares_json).unwrap()).unwrap();

    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&shares_path).unwrap()).unwrap();
    let mut shares = Vec::new();
    for item in raw["shares"].as_array().unwrap() {
        let x = item["x"].as_u64().unwrap() as u8;
        let ys_vec = xavier::crypto::hex_decode(item["ys_hex"].as_str().unwrap()).unwrap();
        let mut ys = [0u8; 32];
        ys.copy_from_slice(&ys_vec);
        shares.push(ShamirShare { x, ys });
    }

    let challenge = OrderedChallenge::new(OrderMode::Asc, &original.check_codes);
    let response = challenge.expected_response(&original.check_codes);
    let recovered = NodeBootstrap::recover_from_shares(
        &shares,
        Some("swal"),
        &response,
        &challenge,
        "999999",
        None,
    )
    .unwrap();

    store.save_vault(&recovered.vault).unwrap();
    store
        .save_public_identity(&PublicNodeIdentity::from_keys(&recovered.keys))
        .unwrap();

    let after = store.load_public_identity().unwrap();
    assert_eq!(after.node_id, before.node_id);
    assert_eq!(after.ed25519_public_hex, before.ed25519_public_hex);
    assert_eq!(after.ml_dsa_commitment_hex, before.ml_dsa_commitment_hex);

    let (_o, keys, _) = store.unlock("999999", None).unwrap();
    assert_eq!(keys.node_id.as_str(), before.node_id);
}
