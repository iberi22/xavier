use std::fs;

use rand::RngCore;
use xavier::crypto::hex_encode;
use xavier::memory::sqlite_vec_store::at_rest;
use xavier::recovery::{
    MnemonicPath, RecoveryStore, RestoreOutcome, MNEMONIC_SEAL_FILE, PASSPHRASE_SEAL_FILE,
};

#[test]
fn test_recovery_unseal_e2e_full_lifecycle() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let base_path = tmp.path().to_path_buf();
    let recovery_dir = base_path.join("recovery");
    let data_dir = base_path.join("data");

    fs::create_dir_all(&recovery_dir).expect("create recovery dir");
    fs::create_dir_all(&data_dir).expect("create data dir");

    // Guard environment variables
    let prev_recovery = std::env::var("XAVIER_RECOVERY_DIR").ok();
    let prev_data = std::env::var("XAVIER_DATA_DIR").ok();
    let prev_home = std::env::var("HOME").ok();
    let prev_key = std::env::var("XAVIER_RECORD_KEY").ok();

    std::env::set_var("XAVIER_RECOVERY_DIR", &recovery_dir);
    std::env::set_var("XAVIER_DATA_DIR", &data_dir);
    std::env::set_var("HOME", &base_path);
    std::env::remove_var("XAVIER_RECORD_KEY");

    struct EnvGuard {
        prev_recovery: Option<String>,
        prev_data: Option<String>,
        prev_home: Option<String>,
        prev_key: Option<String>,
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(v) = &self.prev_recovery {
                std::env::set_var("XAVIER_RECOVERY_DIR", v);
            } else {
                std::env::remove_var("XAVIER_RECOVERY_DIR");
            }
            if let Some(v) = &self.prev_data {
                std::env::set_var("XAVIER_DATA_DIR", v);
            } else {
                std::env::remove_var("XAVIER_DATA_DIR");
            }
            if let Some(v) = &self.prev_home {
                std::env::set_var("HOME", v);
            } else {
                std::env::remove_var("HOME");
            }
            if let Some(v) = &self.prev_key {
                std::env::set_var("XAVIER_RECORD_KEY", v);
            } else {
                std::env::remove_var("XAVIER_RECORD_KEY");
            }
        }
    }
    let _guard = EnvGuard {
        prev_recovery,
        prev_data,
        prev_home,
        prev_key,
    };

    // a. Resolve record key with source (minted fresh in <data_dir>/node/record.key)
    let (resolved_key, source) = at_rest::resolve_record_key_with_source();
    let key = resolved_key.expect("expected key to be generated or resolved");
    let key_path = at_rest::record_key_path();
    assert!(
        key_path.exists(),
        "record.key must exist at {}",
        key_path.display()
    );
    match source {
        at_rest::KeySource::File(_) | at_rest::KeySource::Generated(_) => {}
        other => panic!("unexpected key source: {other:?}"),
    }

    // b. Secret: random dek, wrapped = at_rest::wrap_dek(&dek, &key)
    let mut dek = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut dek);
    let wrapped = at_rest::wrap_dek(&dek, &key).expect("wrap_dek must succeed");
    let unwrapped_initial = at_rest::unwrap_dek(&wrapped, &key).expect("unwrap_dek must succeed");
    assert_eq!(unwrapped_initial, dek);

    // c. store.write_kcv(&key), write_mnemonic_seal, write_passphrase_seal
    let store = RecoveryStore::at(&recovery_dir);
    store.write_kcv(&key).expect("write_kcv");
    let (words, _) = MnemonicPath::generate().expect("generate mnemonic");
    let passphrase = "frase-de-prueba-larga-123";

    let m_seal_path = store
        .write_mnemonic_seal(&key, &words)
        .expect("write_mnemonic_seal");
    let p_seal_path = store
        .write_passphrase_seal(&key, passphrase)
        .expect("write_passphrase_seal");

    // h. Seal files have permissions 0600 on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&m_seal_path).unwrap().permissions().mode() & 0o777,
            0o600,
            "mnemonic seal must have 0600 permissions"
        );
        assert_eq!(
            fs::metadata(&p_seal_path).unwrap().permissions().mode() & 0o777,
            0o600,
            "passphrase seal must have 0600 permissions"
        );
    }
    assert_eq!(m_seal_path, recovery_dir.join(MNEMONIC_SEAL_FILE));
    assert_eq!(p_seal_path, recovery_dir.join(PASSPHRASE_SEAL_FILE));

    let kcv = store.read_kcv().expect("read_kcv");
    assert!(kcv.is_some(), "KCV must be present");

    // d. DELETE record.key
    fs::remove_file(&key_path).expect("delete record.key");
    assert!(!key_path.exists());

    // e. unseal_mnemonic(&words) -> restore_into(hex, &record_key_path(), kcv) -> Installed
    let unsealed_key = store
        .unseal_mnemonic(&words)
        .expect("unseal_mnemonic must succeed");
    assert_eq!(unsealed_key, key);
    let outcome = store
        .restore_into(&hex_encode(unsealed_key), &key_path, kcv.as_deref())
        .expect("restore_into");
    assert_eq!(outcome, RestoreOutcome::Installed);

    // f. at_rest::resolve_record_key() returns same key and unwrap_dek(&wrapped, &k) == dek
    let recovered_key = at_rest::resolve_record_key().expect("resolve_record_key");
    assert_eq!(recovered_key, key);
    let unwrapped = at_rest::unwrap_dek(&wrapped, &recovered_key).expect("unwrap_dek");
    assert_eq!(unwrapped, dek);

    // g. Repeat d-f with passphrase:
    // Delete record.key
    fs::remove_file(&key_path).expect("delete record.key again");
    assert!(!key_path.exists());

    // Unseal with passphrase and restore
    let unsealed_pass = store
        .unseal_passphrase(passphrase)
        .expect("unseal_passphrase must succeed");
    assert_eq!(unsealed_pass, key);
    let outcome_pass = store
        .restore_into(&hex_encode(unsealed_pass), &key_path, kcv.as_deref())
        .expect("restore_into passphrase");
    assert_eq!(outcome_pass, RestoreOutcome::Installed);

    let recovered_key_pass = at_rest::resolve_record_key().expect("resolve_record_key pass");
    assert_eq!(recovered_key_pass, key);
    let unwrapped_pass =
        at_rest::unwrap_dek(&wrapped, &recovered_key_pass).expect("unwrap_dek pass");
    assert_eq!(unwrapped_pass, dek);

    // Wrong words -> error and record.key intact
    let record_key_content_before = fs::read_to_string(&key_path).unwrap();
    let (wrong_words, _) = MnemonicPath::generate().expect("generate wrong words");
    assert!(
        store.unseal_mnemonic(&wrong_words).is_err(),
        "unseal with wrong words must fail"
    );
    assert_eq!(
        fs::read_to_string(&key_path).unwrap(),
        record_key_content_before,
        "record.key must be intact after wrong words"
    );

    // Wrong passphrase -> error and record.key intact
    assert!(
        store
            .unseal_passphrase("wrong-passphrase-does-not-match")
            .is_err(),
        "unseal with wrong passphrase must fail"
    );
    assert_eq!(
        fs::read_to_string(&key_path).unwrap(),
        record_key_content_before,
        "record.key must be intact after wrong passphrase"
    );

    // Second restore with record.key present -> Rejected and file does not change
    let second_outcome = store
        .restore_into(&hex_encode(unsealed_key), &key_path, kcv.as_deref())
        .expect("second restore_into");
    assert!(
        matches!(second_outcome, RestoreOutcome::Rejected { .. }),
        "restoring over existing file must be Rejected"
    );
    assert_eq!(
        fs::read_to_string(&key_path).unwrap(),
        record_key_content_before,
        "record.key must not change on rejected restore"
    );
}
