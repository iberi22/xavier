use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use xavier::data_commons::ivn_storage::{ivn_root, load_json, save_json};
use xavier::data_commons::reputation::KarmaEngine;

xavier::isolate_test_process!();

struct RestoreProcessPaths {
    cwd: PathBuf,
    data_dir: Option<std::ffi::OsString>,
}

impl Drop for RestoreProcessPaths {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.cwd).unwrap();
        match &self.data_dir {
            Some(value) => std::env::set_var("XAVIER_DATA_DIR", value),
            None => std::env::remove_var("XAVIER_DATA_DIR"),
        }
    }
}

#[test]
fn karma_uses_ivn_root_and_imports_legacy_once_without_writing_it() {
    let temporary = tempfile::tempdir().unwrap();
    let _restore = RestoreProcessPaths {
        cwd: std::env::current_dir().unwrap(),
        data_dir: std::env::var_os("XAVIER_DATA_DIR"),
    };
    let cwd = temporary.path().join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    std::env::set_current_dir(&cwd).unwrap();
    std::env::remove_var("XAVIER_DATA_DIR");
    assert_eq!(
        ivn_root().unwrap(),
        dirs::data_dir().unwrap().join("xavier/ivn")
    );
    let data_dir = temporary.path().join("canonical");
    std::env::set_var("XAVIER_DATA_DIR", &data_dir);
    assert_eq!(ivn_root().unwrap(), data_dir.join("ivn"));

    let mut engine = KarmaEngine::new();
    engine.reward("canonical", 19, "path test");
    drop(engine);
    assert!(data_dir.join("ivn/karma.json").is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(data_dir.join("ivn"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    assert!(!cwd.join("data/ivn/karma.json").exists());
    let engine = KarmaEngine::new();
    assert_eq!(engine.get_karma("canonical"), 19);
    drop(engine);

    // The legacy fixture lives entirely in the temporary working directory.
    let legacy = cwd.join("data/ivn/karma.json");
    let mut legacy_engine = KarmaEngine::with_path(&legacy);
    legacy_engine.reward("legacy", 41, "legacy fixture");
    drop(legacy_engine);
    let legacy_bytes = std::fs::read(&legacy).unwrap();
    let imported_dir = temporary.path().join("imported");
    std::env::set_var("XAVIER_DATA_DIR", &imported_dir);
    let mut imported = KarmaEngine::new();
    assert_eq!(imported.get_karma("legacy"), 41);
    assert!(imported_dir.join("ivn/karma.json").is_file());
    imported.reward("legacy", 1, "canonical write");
    drop(imported);
    assert_eq!(std::fs::read(&legacy).unwrap(), legacy_bytes);

    // A changed legacy file cannot override the canonical snapshot on restart.
    std::fs::write(&legacy, b"invalid legacy document").unwrap();
    let reloaded = KarmaEngine::new();
    assert_eq!(reloaded.get_karma("legacy"), 42);
    assert_eq!(std::fs::read(&legacy).unwrap(), b"invalid legacy document");

    let failed_import = temporary.path().join("failed-import");
    std::env::set_var("XAVIER_DATA_DIR", &failed_import);
    let mut failed = KarmaEngine::new();
    failed.reward("legacy", 1, "failed import must stay unavailable");
    failed.decay();
    assert!(failed.save().is_err());
    assert_eq!(std::fs::read(&legacy).unwrap(), b"invalid legacy document");
    assert!(!failed_import.join("ivn/karma.json").exists());
}

#[test]
fn atomic_json_publishes_complete_private_documents() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("store.json");
    let original = json!({"value": "before"});
    let replacement = json!({"value": "after", "padding": "x".repeat(8192)});
    save_json(&path, &original).unwrap();
    let reader_path = path.clone();
    let reader = std::thread::spawn(move || {
        for _ in 0..1000 {
            let document: Value = load_json(&reader_path).unwrap();
            assert!(document["value"] == "before" || document["value"] == "after");
        }
    });
    for _ in 0..20 {
        save_json(&path, &replacement).unwrap();
        save_json(&path, &original).unwrap();
    }
    reader.join().unwrap();
    assert_eq!(load_json::<Value>(&path).unwrap(), original);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 1);
}

struct FailingDocument;

impl Serialize for FailingDocument {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom(
            "intentional serialization failure",
        ))
    }
}

#[test]
fn atomic_json_preserves_previous_file_and_cleans_failed_temporary_files() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("store.json");
    save_json(&path, &json!({"intact": true})).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(save_json(&path, &FailingDocument).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);

    let blocked = temporary.path().join("blocked.json");
    std::fs::create_dir(&blocked).unwrap();
    assert!(save_json(&blocked, &json!({"never": "published"})).is_err());
    assert!(blocked.is_dir());
    assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 2);
}

#[test]
fn json_load_reports_corruption_and_read_errors() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("store.json");
    assert_eq!(
        load_json::<Vec<String>>(&path).unwrap(),
        Vec::<String>::new()
    );
    std::fs::write(&path, b"{").unwrap();
    assert!(load_json::<Value>(&path).is_err());
    assert!(load_json::<Value>(temporary.path()).is_err());
}

#[test]
fn corrupt_karma_is_not_overwritten_after_reward_or_decay() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("karma.json");
    let corrupt = b"invalid karma document";
    std::fs::write(&path, corrupt).unwrap();
    let mut engine = KarmaEngine::with_path(&path);
    engine.reward("agent", 100, "must not overwrite");
    assert_eq!(std::fs::read(&path).unwrap(), corrupt);
    engine.decay();
    assert_eq!(std::fs::read(&path).unwrap(), corrupt);
    assert!(engine.save().is_err());
}

#[test]
fn corrupt_exclusions_are_not_overwritten() {
    use xavier::data_commons::ivn::{clear_exclusions, is_excluded_at, record_exclusion};
    use xavier::data_commons::types::WalletAddress;

    let temporary = tempfile::tempdir().unwrap();
    let _restore = RestoreProcessPaths {
        cwd: std::env::current_dir().unwrap(),
        data_dir: std::env::var_os("XAVIER_DATA_DIR"),
    };
    std::env::set_var("XAVIER_DATA_DIR", temporary.path());
    let root = ivn_root().unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("exclusions.json");
    let corrupt = b"invalid exclusions document";
    std::fs::write(&path, corrupt).unwrap();
    let wallet = WalletAddress("excluded-agent".into());
    assert!(!is_excluded_at(&wallet, 0));
    record_exclusion(&wallet, 100);
    assert_eq!(std::fs::read(&path).unwrap(), corrupt);
    clear_exclusions();
    assert_eq!(std::fs::read(&path).unwrap(), corrupt);
}
