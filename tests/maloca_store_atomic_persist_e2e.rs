//! feat-maloca-store-atomic-persist: verify that MalocaStore persists state atomically without leftover `.tmp` files.
use tempfile::tempdir;
use xavier::maloca::{CreateSupportBody, MalocaStore};

xavier::isolate_test_process!();

#[test]
#[serial_test::serial]
fn persist_leaves_no_tmp_files_after_success_e2e() {
    let tmp = tempdir().unwrap();
    let store = MalocaStore::open(tmp.path());

    // Creating support items triggers a persist
    store.create_support(CreateSupportBody {
        title: "first".into(),
        body: String::new(),
        feature_id: None,
    });
    store.create_support(CreateSupportBody {
        title: "second".into(),
        body: String::new(),
        feature_id: None,
    });

    let leftovers: Vec<String> = std::fs::read_dir(tmp.path().join("maloca"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();

    assert!(
        leftovers.is_empty(),
        "successful persist must not leave temp files behind: {:?}",
        leftovers
    );

    // Verify the store json is written.
    let store_file = tmp.path().join("maloca").join("store.json");
    assert!(store_file.exists());
    let content = std::fs::read_to_string(&store_file).unwrap();
    assert!(content.contains("first"));
    assert!(content.contains("second"));
}

#[test]
#[serial_test::serial]
fn persisted_store_json_is_valid_json_after_write_e2e() {
    let tmp = tempdir().unwrap();
    let store = MalocaStore::open(tmp.path());
    store.create_support(CreateSupportBody {
        title: "test valid json".into(),
        body: String::new(),
        feature_id: None,
    });

    let store_file = tmp.path().join("maloca").join("store.json");
    assert!(store_file.exists());
    let content = std::fs::read_to_string(&store_file).unwrap();

    // It should be valid JSON
    let _parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
}

#[test]
#[serial_test::serial]
fn persist_replaces_store_file_by_rename_not_in_place_e2e() {
    let tmp = tempdir().unwrap();
    let store = MalocaStore::open(tmp.path());

    store.create_support(CreateSupportBody {
        title: "initial".into(),
        body: String::new(),
        feature_id: None,
    });

    let store_file = tmp.path().join("maloca").join("store.json");

    // Open the file *before* the second persist
    let mut file = std::fs::File::open(&store_file).unwrap();

    #[cfg(unix)]
    let inode_before = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(&store_file).unwrap().ino()
    };

    store.create_support(CreateSupportBody {
        title: "subsequent".into(),
        body: String::new(),
        feature_id: None,
    });

    #[cfg(unix)]
    let inode_after = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(&store_file).unwrap().ino()
    };

    #[cfg(unix)]
    assert_ne!(
        inode_before, inode_after,
        "inode must change upon atomic rename"
    );

    // Read from the already-open file handle
    use std::io::Read;
    let mut old_content = String::new();
    file.read_to_string(&mut old_content).unwrap();

    // The old file handle must STILL have the old content (no "subsequent")
    // because atomic replacement writes to a new file and swaps the dir entry,
    // leaving the old inode intact for existing readers.
    assert!(
        !old_content.contains("subsequent"),
        "the already-open file handle must read the old contents, proving it was not overwritten in place"
    );

    let content = std::fs::read_to_string(&store_file).unwrap();
    assert!(content.contains("subsequent"));
}
