#[path = "../src/mesh/keystore.rs"]
mod keystore;

use keystore::MeshKeyringStore;
use tempfile::TempDir;
use xavier::mesh::node::NodeIdentity;

#[test]
fn test_mesh_keystore_fallback_save_load_roundtrip() {
    let temp_dir = TempDir::new().unwrap();
    let store = MeshKeyringStore::with_path(temp_dir.path().to_path_buf());

    println!("is keyring available: {}", store.is_keyring_available());

    let identity = NodeIdentity::generate();
    let node_id = identity.node_id.as_str().to_string();

    // Save key
    store.save_identity(&identity).unwrap();

    // Load key back
    let loaded = store.load_identity(&node_id).unwrap();

    assert_eq!(identity.node_id, loaded.node_id);
    assert_eq!(identity.public_key, loaded.public_key);
    assert_eq!(identity.private_key_bytes(), loaded.private_key_bytes());

    // Test signing and verification consistency
    let msg = b"Test message for keystore integration";
    let sig = loaded.sign(msg);
    assert!(NodeIdentity::verify(&loaded.public_key, msg, &sig));
}

#[test]
fn test_mesh_keystore_delete() {
    let temp_dir = TempDir::new().unwrap();
    let store = MeshKeyringStore::with_path(temp_dir.path().to_path_buf());

    let identity = NodeIdentity::generate();
    let node_id = identity.node_id.as_str().to_string();

    store.save_identity(&identity).unwrap();
    assert!(store.load_key(&node_id).is_ok());

    store.delete_key(&node_id).unwrap();
    assert!(store.load_key(&node_id).is_err());
}

#[test]
fn test_mesh_keystore_probe() {
    let store = MeshKeyringStore::new();
    // probe method should execute without panicking
    let _available = store.is_keyring_available();
}
