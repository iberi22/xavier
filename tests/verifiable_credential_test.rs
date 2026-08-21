use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use xavier::data_commons::verifiable_credential::{
    CredentialError, DatasetCredentialGenerator, DatasetCredentialParams,
};

#[test]
fn test_credential_generation_and_verification() {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();

    let dataset_bytes = b"{\"id\": 1, \"input\": \"hello\", \"output\": \"world\"}\n";
    let dataset_digest = DatasetCredentialGenerator::compute_dataset_digest(dataset_bytes);

    let params = DatasetCredentialParams {
        dataset_id: "ds-12345".to_string(),
        dataset_name: "test-dataset-v1".to_string(),
        dataset_digest: dataset_digest.clone(),
        record_count: 100,
        license: "AGPL-3.0-only".to_string(),
        curation_status: "APPROVED".to_string(),
    };

    let vc = DatasetCredentialGenerator::generate_credential(params, &signing_key)
        .expect("Failed to generate credential");

    assert_eq!(vc.credential_subject.dataset_id, "ds-12345");
    assert_eq!(vc.credential_subject.dataset_digest, dataset_digest);
    assert_eq!(vc.credential_subject.record_count, 100);
    assert_eq!(vc.credential_subject.license, "AGPL-3.0-only");
    assert_eq!(vc.credential_subject.curation_status, "APPROVED");

    let expected_did = DatasetCredentialGenerator::public_key_to_did(&verifying_key);
    assert_eq!(vc.issuer, expected_did);

    // Verify valid VC without content comparison
    let is_valid = DatasetCredentialGenerator::verify_credential(&vc, None)
        .expect("Verification should succeed");
    assert!(is_valid);

    // Verify valid VC with content comparison
    let is_valid_with_content =
        DatasetCredentialGenerator::verify_credential(&vc, Some(dataset_bytes))
            .expect("Verification with content should succeed");
    assert!(is_valid_with_content);
}

#[test]
fn test_credential_json_serialization_roundtrip() {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);

    let params = DatasetCredentialParams {
        dataset_id: "ds-json-test".to_string(),
        dataset_name: "json-dataset".to_string(),
        dataset_digest: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            .to_string(),
        record_count: 42,
        license: "CC-BY-4.0".to_string(),
        curation_status: "APPROVED".to_string(),
    };

    let vc = DatasetCredentialGenerator::generate_credential(params, &signing_key).unwrap();

    let json_str = serde_json::to_string_pretty(&vc).expect("Serialization failed");
    assert!(json_str.contains("\"@context\""));
    assert!(json_str.contains("\"DatasetCredential\""));
    assert!(json_str.contains("\"proof\""));

    let deserialized_vc: xavier::data_commons::verifiable_credential::VerifiableCredential =
        serde_json::from_str(&json_str).expect("Deserialization failed");

    assert_eq!(vc, deserialized_vc);

    let is_valid = DatasetCredentialGenerator::verify_credential(&deserialized_vc, None)
        .expect("Deserialized VC verification failed");
    assert!(is_valid);
}

#[test]
fn test_tamper_detection_modified_subject() {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);

    let params = DatasetCredentialParams {
        dataset_id: "ds-tamper".to_string(),
        dataset_name: "original-dataset".to_string(),
        dataset_digest: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            .to_string(),
        record_count: 500,
        license: "AGPL-3.0-only".to_string(),
        curation_status: "APPROVED".to_string(),
    };

    let mut vc = DatasetCredentialGenerator::generate_credential(params, &signing_key).unwrap();

    // Tamper with record count claim
    vc.credential_subject.record_count = 10000;

    let res = DatasetCredentialGenerator::verify_credential(&vc, None);
    assert!(
        matches!(res, Err(CredentialError::InvalidSignature)),
        "Expected InvalidSignature error after tampering claim, got {:?}",
        res
    );
}

#[test]
fn test_tamper_detection_modified_digest() {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);

    let raw_bytes = b"original raw dataset content";
    let params = DatasetCredentialParams {
        dataset_id: "ds-digest-tamper".to_string(),
        dataset_name: "dataset".to_string(),
        dataset_digest: DatasetCredentialGenerator::compute_dataset_digest(raw_bytes),
        record_count: 10,
        license: "MIT".to_string(),
        curation_status: "APPROVED".to_string(),
    };

    let vc = DatasetCredentialGenerator::generate_credential(params, &signing_key).unwrap();

    let tampered_bytes = b"tampered dataset content!";

    let res = DatasetCredentialGenerator::verify_credential(&vc, Some(tampered_bytes));
    assert!(
        matches!(res, Err(CredentialError::DigestMismatch { .. })),
        "Expected DigestMismatch error, got {:?}",
        res
    );
}

#[test]
fn test_tamper_detection_invalid_signature_proof() {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);

    let params = DatasetCredentialParams {
        dataset_id: "ds-sig-tamper".to_string(),
        dataset_name: "dataset".to_string(),
        dataset_digest: "0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        record_count: 5,
        license: "MIT".to_string(),
        curation_status: "APPROVED".to_string(),
    };

    let mut vc = DatasetCredentialGenerator::generate_credential(params, &signing_key).unwrap();

    if let Some(ref mut proof) = vc.proof {
        // Tamper signature hex string
        proof.proof_value = "0".repeat(128);
    }

    let res = DatasetCredentialGenerator::verify_credential(&vc, None);
    assert!(
        matches!(res, Err(CredentialError::InvalidSignature)),
        "Expected InvalidSignature error, got {:?}",
        res
    );
}
