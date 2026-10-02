//! E2E integration test for the API key generator (`src/secrets/keygen.rs`)
//! running against a real `HardwareVault` instance isolated in a temporary directory.
//!
//! Note on isolation: `HardwareVault::isolated` is `pub(crate)` + `#[cfg(test)]`
//! within the `xavier` crate, making it inaccessible from external integration tests in `tests/`.
//! Therefore, test isolation is achieved via `tempfile::tempdir()` and setting `XAVIER_DATA_DIR`
//! to prevent touching `~/.xavier/secrets`.

use chrono::Utc;
use tempfile::tempdir;

use xavier::secrets::keygen::{
    export_key, fingerprint, format_generate_output, generate_key, load_metadata, revoke_key,
    validate_name, KeyScope,
};
use xavier::secrets::vault::HardwareVault;
use xavier::secrets::SecretError;

fn setup_isolated_vault(service_name: &str) -> (HardwareVault, tempfile::TempDir) {
    let dir = tempdir().expect("failed to create temp directory for vault isolation");
    std::env::set_var("XAVIER_DATA_DIR", dir.path());
    let vault = HardwareVault::new(service_name);
    (vault, dir)
}

fn test_now() -> chrono::DateTime<Utc> {
    chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp")
}

#[test]
fn test_e2e_keygen_persists_and_exports_exact_value() {
    let (vault, _dir) = setup_isolated_vault("xavier-keygen-e2e-persist");
    let name = "service_auth";
    let now = test_now();

    let gen_key =
        generate_key(&vault, name, KeyScope::Live, 3600, now).expect("generate_key failed");
    assert_eq!(gen_key.metadata.name, "service_auth");
    assert_eq!(gen_key.metadata.scope, "live");
    assert_eq!(gen_key.metadata.ttl_secs, 3600);
    assert_eq!(gen_key.metadata.rotation_count, 0);

    let exported = export_key(&vault, name).expect("export_key failed");
    assert_eq!(exported, gen_key.value);
    assert!(
        exported.starts_with("xavier_live_"),
        "key value must start with xavier_live_: {exported}"
    );

    let body = exported
        .strip_prefix("xavier_live_")
        .expect("prefix missing");
    assert_eq!(
        body.len(),
        64,
        "key body entropy must be 64 hex characters, got length {}",
        body.len()
    );
    assert!(
        body.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
        "key body must be lowercase hex"
    );
}

#[test]
fn test_e2e_keygen_export_missing_key_returns_err() {
    let (vault, _dir) = setup_isolated_vault("xavier-keygen-e2e-missing");
    let res = export_key(&vault, "nonexistent_key_name");
    assert!(res.is_err(), "exporting non-existent key must return Err");
    match res {
        Err(SecretError::NotFound(key_name)) => {
            assert!(
                key_name.contains("nonexistent_key_name"),
                "error message should contain requested key name"
            );
        }
        Err(other) => {
            // SecretError::NotFound or ProviderError expected for missing key
            assert!(
                other.to_string().to_lowercase().contains("not found")
                    || other.to_string().to_lowercase().contains("missing"),
                "unexpected error: {other}"
            );
        }
        Ok(val) => panic!("expected Err for missing key, got Ok({val:?})"),
    }
}

#[test]
fn test_e2e_keygen_rotation_increments_count_and_replaces_value() {
    let (vault, _dir) = setup_isolated_vault("xavier-keygen-e2e-rotate");
    let name = "rotatable_api_key";
    let now = test_now();

    let first = generate_key(&vault, name, KeyScope::Live, 86400, now).expect("first generation");
    assert_eq!(first.metadata.rotation_count, 0);

    let second =
        generate_key(&vault, name, KeyScope::Live, 86400, now).expect("second generation");
    assert_eq!(second.metadata.rotation_count, 1);
    assert_ne!(first.value, second.value, "rotated key value must change");

    let third = generate_key(&vault, name, KeyScope::Live, 86400, now).expect("third generation");
    assert_eq!(third.metadata.rotation_count, 2);
    assert_ne!(second.value, third.value, "rotated key value must change");

    // Only the newest rotated key value remains active in the vault
    let exported = export_key(&vault, name).expect("export_key after rotation");
    assert_eq!(exported, third.value);

    let meta = load_metadata(&vault, name).expect("metadata present after rotation");
    assert_eq!(meta.rotation_count, 2);
    assert_eq!(meta.fingerprint, third.metadata.fingerprint);
}

#[test]
fn test_e2e_keygen_format_output_never_contains_secret_value() {
    let (vault, _dir) = setup_isolated_vault("xavier-keygen-e2e-format");
    let name = "redacted_key";
    let now = test_now();

    let key = generate_key(&vault, name, KeyScope::Live, 7200, now).expect("generate_key");
    let formatted = format_generate_output(&key);

    assert!(
        !formatted.contains(&key.value),
        "formatted output MUST NOT contain full key value; got: {formatted}"
    );

    // Assert exact elements in format string
    assert!(formatted.contains("redacted_key"));
    assert!(formatted.contains("env=live"));
    assert!(formatted.contains("backend=hardware-vault"));
    assert!(formatted.contains(&format!("fingerprint={}", key.metadata.fingerprint)));
    assert!(formatted.contains("ttl_secs=7200"));
    assert!(formatted.contains("rotation=0"));
    assert!(formatted.contains("value=<hidden>"));

    // Verify key body payload substring is not present
    let body = &key.value["xavier_live_".len()..];
    assert!(
        !formatted.contains(&body[..16]),
        "key body substring leaked in output"
    );
}

#[test]
fn test_e2e_keygen_fingerprint_stability_and_format() {
    let sample_value = "xavier_live_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let fp1 = fingerprint(sample_value);
    let fp2 = fingerprint(sample_value);

    assert_eq!(fp1, fp2, "fingerprint must be deterministic");
    assert!(
        fp1.starts_with("sha256:"),
        "fingerprint must start with sha256: prefix, got: {fp1}"
    );

    let hex_part = fp1.strip_prefix("sha256:").expect("sha256: prefix missing");
    assert_eq!(
        hex_part.len(),
        16,
        "fingerprint hex string length must be exactly 16 chars, got: {}",
        hex_part.len()
    );
    assert!(
        hex_part
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
        "fingerprint hex string must be lowercase hex"
    );
}

#[test]
fn test_e2e_keygen_revoke_deletes_value_and_metadata() {
    let (vault, _dir) = setup_isolated_vault("xavier-keygen-e2e-revoke");
    let name = "ephemeral_token";
    let now = test_now();

    generate_key(&vault, name, KeyScope::Test, 1800, now).expect("generate_key");
    assert!(export_key(&vault, name).is_ok());
    assert!(load_metadata(&vault, name).is_some());

    revoke_key(&vault, name).expect("revoke_key should succeed on existing key");

    assert!(
        export_key(&vault, name).is_err(),
        "exported key after revocation must return Err"
    );
    assert!(
        load_metadata(&vault, name).is_none(),
        "metadata after revocation must be None"
    );

    // Second revoke call on missing key must fail
    assert!(
        revoke_key(&vault, name).is_err(),
        "second revoke_key call on missing key must fail"
    );
}

#[test]
fn test_e2e_keygen_validate_name_rejects_traversal_and_invalid_chars() {
    let bad_names = [
        "",
        "..",
        "../etc/passwd",
        "a/b",
        "with space",
        &"a".repeat(65),
    ];

    for bad_name in bad_names {
        let res = validate_name(bad_name);
        assert!(
            res.is_err(),
            "validate_name should reject bad name: {bad_name:?}"
        );
    }

    let good_names = ["valid_key", "valid-key-123", "KEY_NAME", "a"];
    for good_name in good_names {
        assert!(
            validate_name(good_name).is_ok(),
            "validate_name should accept good name: {good_name:?}"
        );
    }
}

#[test]
fn test_e2e_keygen_scope_prefixes_and_64_hex_chars() {
    let (vault, _dir) = setup_isolated_vault("xavier-keygen-e2e-scopes");
    let now = test_now();

    let live_key =
        generate_key(&vault, "live_app", KeyScope::Live, 3600, now).expect("generate live key");
    let test_key =
        generate_key(&vault, "test_app", KeyScope::Test, 3600, now).expect("generate test key");

    assert!(live_key.value.starts_with("xavier_live_"));
    assert!(test_key.value.starts_with("xavier_test_"));

    let live_body = live_key
        .value
        .strip_prefix("xavier_live_")
        .expect("strip live prefix");
    let test_body = test_key
        .value
        .strip_prefix("xavier_test_")
        .expect("strip test prefix");

    assert_eq!(live_body.len(), 64);
    assert_eq!(test_body.len(), 64);

    assert!(live_body.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(test_body.chars().all(|c| c.is_ascii_hexdigit()));
}
