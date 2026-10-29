//! feat-maloca-store-atomic-persist: verify that MalocaStore persists state atomically without leftover `.tmp` files.
use tempfile::tempdir;
use xavier::maloca::{CreateSupportBody, MalocaStore};

xavier::isolate_test_process!();

#[test]
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
fn persist_replaces_store_file_by_rename_not_in_place_e2e() {
    let tmp = tempdir().unwrap();
    let store = MalocaStore::open(tmp.path());

    store.create_support(CreateSupportBody {
        title: "initial".into(),
        body: String::new(),
        feature_id: None,
    });

    let store_file = tmp.path().join("maloca").join("store.json");
    // Hold a handle on the original file: an atomic rename-replace leaves the
    // old inode (and its contents) intact for this reader, while an in-place
    // write would truncate/overwrite what the handle sees.
    let mut old_handle = std::fs::File::open(&store_file).unwrap();
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
    {
        use std::os::unix::fs::MetadataExt;
        let inode_after = std::fs::metadata(&store_file).unwrap().ino();
        assert_ne!(
            inode_before, inode_after,
            "persist must replace store.json via rename, not rewrite it in place"
        );
    }

    let mut old_content = String::new();
    std::io::Read::read_to_string(&mut old_handle, &mut old_content).unwrap();
    assert!(old_content.contains("initial"), "{old_content}");
    assert!(
        !old_content.contains("subsequent"),
        "the pre-existing file must be untouched by the replacement: {old_content}"
    );

    let content = std::fs::read_to_string(&store_file).unwrap();
    assert!(content.contains("initial"));
    assert!(content.contains("subsequent"));
}
