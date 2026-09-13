//! E2E Integration Test for Polygon Karma Merkle Rollup Pipeline.
//!
//! Validates the full chain:
//! 1. Synthetic Karma ledger state generation & 32-byte Merkle root construction.
//! 2. Polygon Anchor calldata preparation with `AnchorKind::KarmaRollup` (selector `0x8979b0c2`).
//! 3. Mock broadcast roundtrip simulation with `MockAnchorTransport` and receipt verification.
//! 4. Negative test validation for malformed/unpadded Merkle roots and invalid contract addresses.

use sha2::{Digest, Sha256};
use xavier::crypto::hex_encode;
use xavier::polygon_anchor::abi::{
    encode_anchor_calldata, prepare_anchor_call, AnchorKind, SELECTOR_ANCHOR_KARMA_ROLLUP,
};
use xavier::polygon_anchor::{
    AnchorRegistry, AnchorTransport, MockAnchorTransport, DEFAULT_CHAIN_ID_AMOY,
};

/// Synthetic Karma state transition record.
#[derive(Debug, Clone)]
struct KarmaUpdate {
    node_id: String,
    delta: i64,
    reason: String,
    timestamp: u64,
}

/// Computes a leaf hash for a Karma state transition.
fn compute_leaf_hash(update: &KarmaUpdate) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(
        format!(
            "karma_update:v1|{}|{}|{}|{}",
            update.node_id, update.delta, update.reason, update.timestamp
        )
        .as_bytes(),
    );
    hasher.finalize().into()
}

/// Constructs a 32-byte Merkle tree root from a batch of Karma state updates.
fn compute_merkle_root(updates: &[KarmaUpdate]) -> String {
    if updates.is_empty() {
        let mut hasher = Sha256::new();
        hasher.update(b"empty_karma_batch");
        return hex_encode(hasher.finalize());
    }

    let mut current_level: Vec<[u8; 32]> = updates.iter().map(compute_leaf_hash).collect();

    while current_level.len() > 1 {
        let mut next_level = Vec::new();
        for chunk in current_level.chunks(2) {
            let left = chunk[0];
            let right = if chunk.len() > 1 { chunk[1] } else { chunk[0] };
            let mut hasher = Sha256::new();
            hasher.update(left);
            hasher.update(right);
            next_level.push(hasher.finalize().into());
        }
        current_level = next_level;
    }

    hex_encode(current_level[0])
}

#[test]
fn test_e2e_karma_merkle_root_generation_and_calldata_preparation() {
    let batch = vec![
        KarmaUpdate {
            node_id: "xv1-node-alpha".to_string(),
            delta: 15,
            reason: "valid_block_attestation".to_string(),
            timestamp: 1711000000,
        },
        KarmaUpdate {
            node_id: "xv1-node-beta".to_string(),
            delta: -5,
            reason: "slash_latency_penalty".to_string(),
            timestamp: 1711000005,
        },
        KarmaUpdate {
            node_id: "xv1-node-gamma".to_string(),
            delta: 50,
            reason: "consensus_validator_reward".to_string(),
            timestamp: 1711000010,
        },
        KarmaUpdate {
            node_id: "xv1-node-delta".to_string(),
            delta: 10,
            reason: "relay_uptime_bonus".to_string(),
            timestamp: 1711000015,
        },
    ];

    let merkle_root_hex = compute_merkle_root(&batch);
    assert_eq!(
        merkle_root_hex.len(),
        64,
        "Merkle root must be 64 hex characters (32 bytes)"
    );

    let calldata =
        encode_anchor_calldata(AnchorKind::KarmaRollup, &merkle_root_hex).expect("encode calldata");

    assert_eq!(calldata.len(), 36, "Calldata must be exactly 36 bytes");
    assert_eq!(
        &calldata[0..4],
        &SELECTOR_ANCHOR_KARMA_ROLLUP,
        "Calldata selector must match 0x8979b0c2"
    );
    assert_eq!(
        hex_encode(&calldata[4..]),
        merkle_root_hex,
        "Calldata payload must contain exact 32-byte Merkle root"
    );

    let contract_address = "0x8888888888888888888888888888888888888888";
    let prepared = prepare_anchor_call(
        contract_address,
        &merkle_root_hex,
        DEFAULT_CHAIN_ID_AMOY,
        AnchorKind::KarmaRollup,
    )
    .expect("prepare anchor call");

    assert_eq!(prepared.to, contract_address);
    assert_eq!(prepared.chain_id, DEFAULT_CHAIN_ID_AMOY);
    assert_eq!(prepared.kind, AnchorKind::KarmaRollup);
    assert!(
        prepared.data_hex.starts_with("0x8979b0c2"),
        "Prepared data hex must start with 0x8979b0c2"
    );
}

#[test]
fn test_e2e_mock_broadcast_roundtrip_and_receipt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let registry = AnchorRegistry::under_data_dir(dir.path());
    let transport = MockAnchorTransport;

    let batch = vec![
        KarmaUpdate {
            node_id: "xv1-validator-01".to_string(),
            delta: 100,
            reason: "epoch_finalization".to_string(),
            timestamp: 1711000100,
        },
        KarmaUpdate {
            node_id: "xv1-validator-02".to_string(),
            delta: 100,
            reason: "epoch_finalization".to_string(),
            timestamp: 1711000100,
        },
    ];

    let merkle_root_hex = compute_merkle_root(&batch);
    let contract_address = "0x1111111111111111111111111111111111111111";

    let receipt = transport
        .submit_hash(
            &merkle_root_hex,
            DEFAULT_CHAIN_ID_AMOY,
            Some(contract_address),
        )
        .expect("submit hash mock transport");

    assert!(receipt.dry_run, "Mock transport receipt must be dry_run");
    assert_eq!(receipt.content_hash_hex, merkle_root_hex);
    assert_eq!(receipt.chain_id, DEFAULT_CHAIN_ID_AMOY);
    assert_eq!(receipt.contract.as_deref(), Some(contract_address));
    assert!(
        receipt.tx_hash.starts_with("mock:"),
        "Tx hash must have mock prefix"
    );

    registry.save(&receipt).expect("save receipt to registry");
    let reloaded = registry
        .load(&merkle_root_hex)
        .expect("load receipt from registry");

    assert_eq!(reloaded, receipt);

    // Verify prepared transport roundtrip for KarmaRollup anchor kind
    let env_transport = xavier::polygon_anchor::EnvAnchorTransport {
        rpc_url: Some("https://rpc.amoy.polygon.technology".into()),
        chain_id: DEFAULT_CHAIN_ID_AMOY,
        contract: Some(contract_address.to_string()),
        dry_run: false,
        has_anchor_key: true,
        kind: AnchorKind::KarmaRollup,
    };

    let live_prep_receipt = env_transport
        .submit_hash(&merkle_root_hex, DEFAULT_CHAIN_ID_AMOY, None)
        .expect("submit hash env transport");

    assert!(!live_prep_receipt.dry_run);
    assert!(
        live_prep_receipt.tx_hash.starts_with("live-prepared:"),
        "Tx hash must have live-prepared prefix when SWAL_ANCHOR_BROADCAST is unset"
    );

    let calldata = live_prep_receipt
        .calldata_hex
        .as_ref()
        .expect("calldata_hex present");
    assert!(
        calldata.starts_with("0x8979b0c2"),
        "Prepared calldata must start with KarmaRollup selector 0x8979b0c2"
    );
}

#[test]
fn test_e2e_negative_malformed_merkle_root_and_invalid_contract() {
    // 1. Short / unpadded Merkle root (less than 32 bytes / 64 hex chars)
    let short_root = "0x1234567890abcdef";
    let err_short = encode_anchor_calldata(AnchorKind::KarmaRollup, short_root);
    assert!(err_short.is_err(), "Short Merkle root hex must be rejected");

    // 2. Non-hex characters in Merkle root
    let invalid_hex_root = "Z".repeat(64);
    let err_hex = encode_anchor_calldata(AnchorKind::KarmaRollup, &invalid_hex_root);
    assert!(
        err_hex.is_err(),
        "Invalid hex characters in Merkle root must be rejected"
    );

    // 3. Invalid contract address missing "0x" prefix
    let valid_root = "ab".repeat(32);
    let err_contract_prefix = prepare_anchor_call(
        "1234567890123456789012345678901234567890",
        &valid_root,
        DEFAULT_CHAIN_ID_AMOY,
        AnchorKind::KarmaRollup,
    );
    assert!(
        err_contract_prefix.is_err(),
        "Contract address without 0x prefix must be rejected"
    );

    // 4. Empty contract address
    let err_contract_empty = prepare_anchor_call(
        "",
        &valid_root,
        DEFAULT_CHAIN_ID_AMOY,
        AnchorKind::KarmaRollup,
    );
    assert!(
        err_contract_empty.is_err(),
        "Empty contract address must be rejected"
    );
}
