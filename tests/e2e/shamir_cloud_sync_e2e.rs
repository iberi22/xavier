//! E2E — Shamir Share-3 Cloud Sync & Recovery Lifecycle Tests.
//!
//! Validates full lifecycle of 2-of-3 Shamir split, cloud Share-3 Argon2id + AES-256-GCM
//! serialization, index guards (enforcing share_index == 3), negative tampering/passphrase validation,
//! and threshold reconstruction of root identity keys.

use xavier::crypto::{hex_decode, hex_encode};
use xavier::node_identity::{
    decrypt_cloud_share_3, EncryptedCloudShare, NodeBootstrap, NodeStore, NodeStorePaths,
    OrderMode, OrderedChallenge, PublicNodeIdentity, ShamirSplit,
};

#[test]
fn test_e2e_shamir_cloud_share_prepare_and_recovery_flow() {
    let dir = tempfile::tempdir().unwrap();
    let store = NodeStore::new(NodeStorePaths::from_data_dir(dir.path()));

    let cloud_passphrase = "super-secret-cloud-backup-passphrase-2025";
    let wallet_pin = "e2e-pin-778899";

    // 1. Bootstrap node with 2-of-3 Shamir split
    let created =
        NodeBootstrap::create(Some("bip39-cloud-mnemonic-passphrase"), wallet_pin, None).unwrap();
    let pub_identity = PublicNodeIdentity::from_keys(&created.keys);
    store.save_vault(&created.vault).unwrap();
    store.save_public_identity(&pub_identity).unwrap();

    assert_eq!(created.shares.len(), 3);
    assert_eq!(created.shares[2].x, 3);

    // 2. Prepare Cloud Share-3 encryption with passphrase
    let encrypted_share_3: EncryptedCloudShare = NodeStore::prepare_cloud_share_3(
        &created.shares[2],
        cloud_passphrase,
        created.keys.node_id.as_str(),
    )
    .unwrap();

    assert_eq!(encrypted_share_3.version, 1);
    assert_eq!(encrypted_share_3.node_id, created.keys.node_id.as_str());
    assert_eq!(encrypted_share_3.share_index, 3);
    assert!(!encrypted_share_3.salt_hex.is_empty());
    assert!(!encrypted_share_3.nonce_hex.is_empty());
    assert!(!encrypted_share_3.ciphertext_hex.is_empty());

    // 3. Decrypt Share-3 with correct passphrase
    let decrypted_share_3 = decrypt_cloud_share_3(&encrypted_share_3, cloud_passphrase).unwrap();
    assert_eq!(decrypted_share_3, created.shares[2]);

    // 4. Reconstruct root key using local Share-1 and decrypted cloud Share-3
    let local_share_1 = created.shares[0].clone();
    assert_eq!(local_share_1.x, 1);

    let recovery_shares = vec![local_share_1, decrypted_share_3];
    let reconstructed_entropy = ShamirSplit::combine(&recovery_shares).unwrap();

    let challenge = OrderedChallenge::new(OrderMode::Desc, &created.check_codes);
    let response = challenge.expected_response(&created.check_codes);

    let recovered_bundle = NodeBootstrap::recover_from_shares(
        &recovery_shares,
        Some("bip39-cloud-mnemonic-passphrase"),
        &response,
        &challenge,
        "new-pin-998877",
        None,
    )
    .unwrap();

    assert_eq!(recovered_bundle.keys.node_id, created.keys.node_id);
    assert_eq!(
        recovered_bundle.keys.ed25519_public,
        created.keys.ed25519_public
    );
    assert_eq!(
        recovered_bundle.keys.ml_dsa_commitment,
        created.keys.ml_dsa_commitment
    );

    // Reconstructed entropy matches initial created entropy
    let direct_entropy = ShamirSplit::combine(&created.shares[0..2]).unwrap();
    assert_eq!(reconstructed_entropy, direct_entropy);
}

#[test]
fn test_e2e_shamir_cloud_share_index_guard() {
    let cloud_passphrase = "cloud-passphrase-index-guard";
    let created = NodeBootstrap::create(None, "pin-111222", None).unwrap();
    let node_id = created.keys.node_id.as_str();

    let share_1 = &created.shares[0];
    let share_2 = &created.shares[1];
    let share_3 = &created.shares[2];

    assert_eq!(share_1.x, 1);
    assert_eq!(share_2.x, 2);
    assert_eq!(share_3.x, 3);

    // Enforce share_index == 3 on preparation: Share-1 and Share-2 must fail
    let prep_share_1_res = NodeStore::prepare_cloud_share_3(share_1, cloud_passphrase, node_id);
    assert!(
        prep_share_1_res.is_err(),
        "prepare_cloud_share_3 must reject Share-1"
    );

    let prep_share_2_res = NodeStore::prepare_cloud_share_3(share_2, cloud_passphrase, node_id);
    assert!(
        prep_share_2_res.is_err(),
        "prepare_cloud_share_3 must reject Share-2"
    );

    // Preparation succeeds for Share-3
    let encrypted_share_3 =
        NodeStore::prepare_cloud_share_3(share_3, cloud_passphrase, node_id).unwrap();

    // Enforce share_index == 3 on decryption: tampered share_index must fail
    let mut tampered_share_1 = encrypted_share_3.clone();
    tampered_share_1.share_index = 1;
    let dec_tampered_1_res = decrypt_cloud_share_3(&tampered_share_1, cloud_passphrase);
    assert!(
        dec_tampered_1_res.is_err(),
        "decrypt_cloud_share_3 must reject share_index != 3"
    );

    let mut tampered_share_2 = encrypted_share_3.clone();
    tampered_share_2.share_index = 2;
    let dec_tampered_2_res = decrypt_cloud_share_3(&tampered_share_2, cloud_passphrase);
    assert!(
        dec_tampered_2_res.is_err(),
        "decrypt_cloud_share_3 must reject share_index != 3"
    );

    let mut tampered_share_invalid = encrypted_share_3;
    tampered_share_invalid.share_index = 99;
    let dec_tampered_invalid_res = decrypt_cloud_share_3(&tampered_share_invalid, cloud_passphrase);
    assert!(
        dec_tampered_invalid_res.is_err(),
        "decrypt_cloud_share_3 must reject invalid share_index"
    );
}

#[test]
fn test_e2e_shamir_cloud_share_negative_tampering_and_passphrase_validation() {
    let correct_passphrase = "ultra-secure-cloud-passphrase-2025";
    let wrong_passphrase = "wrong-cloud-passphrase";
    let created = NodeBootstrap::create(Some("negative-test-pass"), "pin-333444", None).unwrap();
    let node_id = created.keys.node_id.as_str();

    let share_3 = &created.shares[2];
    let encrypted = NodeStore::prepare_cloud_share_3(share_3, correct_passphrase, node_id).unwrap();

    // 1. Invalid passphrase must return error and refuse recovery
    let wrong_pass_res = decrypt_cloud_share_3(&encrypted, wrong_passphrase);
    assert!(
        wrong_pass_res.is_err(),
        "decryption with wrong passphrase must fail"
    );

    // 2. Corrupted ciphertext must return error
    let mut corrupted_ct = encrypted.clone();
    let mut ct_bytes = hex_decode(&corrupted_ct.ciphertext_hex).unwrap();
    if let Some(first_byte) = ct_bytes.first_mut() {
        *first_byte ^= 0xFF; // flip bits
    }
    corrupted_ct.ciphertext_hex = hex_encode(ct_bytes);
    let corrupted_ct_res = decrypt_cloud_share_3(&corrupted_ct, correct_passphrase);
    assert!(
        corrupted_ct_res.is_err(),
        "decryption of corrupted ciphertext must fail"
    );

    // 3. Altered salt must return error
    let mut altered_salt = encrypted.clone();
    let mut salt_bytes = hex_decode(&altered_salt.salt_hex).unwrap();
    salt_bytes[0] ^= 0x01;
    altered_salt.salt_hex = hex_encode(salt_bytes);
    let altered_salt_res = decrypt_cloud_share_3(&altered_salt, correct_passphrase);
    assert!(
        altered_salt_res.is_err(),
        "decryption with altered salt must fail"
    );

    // 4. Altered nonce must return error
    let mut altered_nonce = encrypted.clone();
    let mut nonce_bytes = hex_decode(&altered_nonce.nonce_hex).unwrap();
    nonce_bytes[0] ^= 0x01;
    altered_nonce.nonce_hex = hex_encode(nonce_bytes);
    let altered_nonce_res = decrypt_cloud_share_3(&altered_nonce, correct_passphrase);
    assert!(
        altered_nonce_res.is_err(),
        "decryption with altered nonce must fail"
    );

    // 5. Invalid salt / nonce length or invalid hex encoding
    let mut invalid_salt_len = encrypted.clone();
    invalid_salt_len.salt_hex = "1234".to_string(); // 2 bytes instead of 16
    assert!(decrypt_cloud_share_3(&invalid_salt_len, correct_passphrase).is_err());

    let mut invalid_nonce_len = encrypted.clone();
    invalid_nonce_len.nonce_hex = "1234".to_string(); // 2 bytes instead of 12
    assert!(decrypt_cloud_share_3(&invalid_nonce_len, correct_passphrase).is_err());

    let mut invalid_hex = encrypted;
    invalid_hex.ciphertext_hex = "NOT_A_VALID_HEX_STRING!@#$".to_string();
    assert!(decrypt_cloud_share_3(&invalid_hex, correct_passphrase).is_err());
}
