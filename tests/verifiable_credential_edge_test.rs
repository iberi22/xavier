use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use xavier::crypto::hex_encode;
use xavier::data_commons::verifiable_credential::{
    CredentialError, DatasetCredentialGenerator, DatasetCredentialParams,
};

#[test]
fn test_credential_error_display_and_trait() {
    let err_sig = CredentialError::InvalidSignature;
    assert_eq!(
        format!("{err_sig}"),
        "Cryptographic signature verification failed"
    );

    let err_digest = CredentialError::DigestMismatch {
        expected: "abc".to_string(),
        actual: "def".to_string(),
    };
    assert_eq!(
        format!("{err_digest}"),
        "Dataset digest mismatch: expected abc, got def"
    );

    let err_pk = CredentialError::InvalidPublicKey("bad key".to_string());
    assert_eq!(format!("{err_pk}"), "Invalid public key: bad key");

    let err_fmt = CredentialError::InvalidSignatureFormat("bad hex".to_string());
    assert_eq!(format!("{err_fmt}"), "Invalid signature format: bad hex");

    let err_ser = CredentialError::SerializationError("ser err".to_string());
    assert_eq!(format!("{err_ser}"), "Serialization error: ser err");

    let err_malformed = CredentialError::MalformedCredential("missing field".to_string());
    assert_eq!(
        format!("{err_malformed}"),
        "Malformed credential: missing field"
    );

    // Ensure it implements std::error::Error
    let std_err: &dyn std::error::Error = &err_sig;
    assert!(std_err.source().is_none());
}

#[test]
fn test_did_to_public_key_edge_cases() {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();
    let did_with_fragment = format!(
        "{}#key-1",
        DatasetCredentialGenerator::public_key_to_did(&verifying_key)
    );

    // Valid parsing with fragment #key-1
    let parsed_key = DatasetCredentialGenerator::did_to_public_key(&did_with_fragment)
        .expect("Should parse DID with key fragment");
    assert_eq!(parsed_key, verifying_key);

    // Valid parsing without did:swal: prefix
    let hex_pk = hex_encode(verifying_key.as_bytes());
    let parsed_no_prefix =
        DatasetCredentialGenerator::did_to_public_key(&hex_pk).expect("Should parse bare hex DID");
    assert_eq!(parsed_no_prefix, verifying_key);

    // Invalid hex string
    let res_hex_err = DatasetCredentialGenerator::did_to_public_key("did:swal:not_hex_zz!");
    assert!(matches!(
        res_hex_err,
        Err(CredentialError::InvalidPublicKey(_))
    ));

    // Wrong byte length (16 bytes = 32 hex chars)
    let res_short_len =
        DatasetCredentialGenerator::did_to_public_key("did:swal:00112233445566778899aabbccddeeff");
    assert!(
        matches!(res_short_len, Err(CredentialError::InvalidPublicKey(msg)) if msg.contains("Expected 32 bytes"))
    );

    // Wrong byte length (64 bytes = 128 hex chars)
    let res_long_len = DatasetCredentialGenerator::did_to_public_key(&"00".repeat(64));
    assert!(
        matches!(res_long_len, Err(CredentialError::InvalidPublicKey(msg)) if msg.contains("Expected 32 bytes"))
    );

    // Find bytes that fail Ed25519 key parse (point decompression failure)
    let invalid_pk_bytes = [2u8; 32];
    let invalid_pk_hex = hex_encode(&invalid_pk_bytes);
    let res_invalid_pk =
        DatasetCredentialGenerator::did_to_public_key(&format!("did:swal:{invalid_pk_hex}"));
    assert!(
        matches!(res_invalid_pk, Err(CredentialError::InvalidPublicKey(msg)) if msg.contains("Ed25519 key parse error"))
    );
}

#[test]
fn test_verify_credential_edge_cases() {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);

    let params = DatasetCredentialParams {
        dataset_id: "ds-edge-1".to_string(),
        dataset_name: "edge-dataset".to_string(),
        dataset_digest: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            .to_string(),
        record_count: 1,
        license: "MIT".to_string(),
        curation_status: "APPROVED".to_string(),
    };

    let vc = DatasetCredentialGenerator::generate_credential(params, &signing_key)
        .expect("Generation should succeed");

    // Case 1: Missing proof
    let mut vc_no_proof = vc.clone();
    vc_no_proof.proof = None;
    let res_no_proof = DatasetCredentialGenerator::verify_credential(&vc_no_proof, None);
    assert!(matches!(
        res_no_proof,
        Err(CredentialError::MalformedCredential(msg)) if msg.contains("Missing proof")
    ));

    // Case 2: Unsupported proof type
    let mut vc_bad_proof_type = vc.clone();
    if let Some(ref mut proof) = vc_bad_proof_type.proof {
        proof.proof_type = "RsaSignature2018".to_string();
    }
    let res_bad_proof_type =
        DatasetCredentialGenerator::verify_credential(&vc_bad_proof_type, None);
    assert!(matches!(
        res_bad_proof_type,
        Err(CredentialError::MalformedCredential(msg)) if msg.contains("Unsupported proof type")
    ));

    // Case 3: Invalid signature hex string (non-hex characters)
    let mut vc_invalid_hex = vc.clone();
    if let Some(ref mut proof) = vc_invalid_hex.proof {
        proof.proof_value = "invalid_hex_string_zz!".to_string();
    }
    let res_invalid_hex = DatasetCredentialGenerator::verify_credential(&vc_invalid_hex, None);
    assert!(matches!(
        res_invalid_hex,
        Err(CredentialError::InvalidSignatureFormat(_))
    ));

    // Case 4: Invalid signature byte count (e.g. 32 bytes instead of 64 bytes)
    let mut vc_short_sig = vc.clone();
    if let Some(ref mut proof) = vc_short_sig.proof {
        proof.proof_value = "00".repeat(32);
    }
    let res_short_sig = DatasetCredentialGenerator::verify_credential(&vc_short_sig, None);
    assert!(matches!(
        res_short_sig,
        Err(CredentialError::InvalidSignatureFormat(msg)) if msg.contains("Expected 64 bytes")
    ));

    // Case 5: Invalid issuer DID
    let mut vc_bad_issuer = vc.clone();
    vc_bad_issuer.issuer = "did:swal:invalid_did_key".to_string();
    let res_bad_issuer = DatasetCredentialGenerator::verify_credential(&vc_bad_issuer, None);
    assert!(matches!(
        res_bad_issuer,
        Err(CredentialError::InvalidPublicKey(_))
    ));

    // Case 6: Verification with another key (wrong signature)
    let other_key = SigningKey::generate(&mut csprng);
    let other_did = DatasetCredentialGenerator::public_key_to_did(&other_key.verifying_key());
    let mut vc_wrong_key = vc.clone();
    vc_wrong_key.issuer = other_did; // change issuer key without changing signature
    let res_wrong_key = DatasetCredentialGenerator::verify_credential(&vc_wrong_key, None);
    assert!(matches!(
        res_wrong_key,
        Err(CredentialError::InvalidSignature)
    ));
}

#[test]
fn test_multi_gb_and_large_dataset_digest_hashing() {
    // Generate a 10 MB payload in memory to test large dataset digest computation
    let chunk = vec![0x42u8; 1_000_000]; // 1MB
    let mut large_dataset = Vec::with_capacity(10_000_000);
    for _ in 0..10 {
        large_dataset.extend_from_slice(&chunk);
    }

    let digest = DatasetCredentialGenerator::compute_dataset_digest(&large_dataset);
    assert_eq!(digest.len(), 64);

    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);

    let params = DatasetCredentialParams {
        dataset_id: "ds-large-10mb".to_string(),
        dataset_name: "large-10mb-dataset".to_string(),
        dataset_digest: digest.clone(),
        record_count: 100_000,
        license: "AGPL-3.0-only".to_string(),
        curation_status: "APPROVED".to_string(),
    };

    let vc = DatasetCredentialGenerator::generate_credential(params, &signing_key)
        .expect("Credential generation for large dataset should succeed");

    let is_valid = DatasetCredentialGenerator::verify_credential(&vc, Some(&large_dataset))
        .expect("Verification with 10MB payload content should succeed");
    assert!(is_valid);
}

#[test]
fn test_default_context() {
    let ctx = DatasetCredentialGenerator::default_context();
    assert_eq!(ctx.len(), 2);
    assert_eq!(ctx[0], "https://www.w3.org/ns/credentials/v2");
    assert_eq!(ctx[1], "https://schema.org");
}
