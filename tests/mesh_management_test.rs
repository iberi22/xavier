use tempfile::tempdir;
use xavier::enterprise::rbac::Role;
use xavier::memory::schema::ClearanceLevel;
use xavier::mesh::{acl::MeshAcl, node::NodeId};

#[tokio::test]
async fn test_mesh_acl_management() {
    let dir = tempdir().unwrap();
    let acl_path = dir.path().join("mesh_acl.json");
    let mut acl = MeshAcl::load_from(acl_path.clone()).unwrap();

    let node_id = NodeId("test-node-1".to_string());

    // Initially no entry
    assert!(acl.get_entry(&node_id).is_none());

    // Set entry
    acl.set_entry(
        node_id.clone(),
        xavier::mesh::acl::NodeAclEntry {
            role: Role::Editor,
            clearance: ClearanceLevel::Secret,
            namespaces: Some(vec!["workspace:test".to_string()]),
            public_key_hex: "test-public-key".to_string(),
        },
    )
    .unwrap();

    // Verify
    let entry = acl.get_entry(&node_id).unwrap();
    assert_eq!(entry.role, Role::Editor);
    assert_eq!(entry.clearance, ClearanceLevel::Secret);
    assert_eq!(
        entry.namespaces.as_deref(),
        Some(&["workspace:test".to_string()][..])
    );
    assert_eq!(entry.public_key_hex, "test-public-key");

    // Reload and verify persistence
    let reloaded_acl = MeshAcl::load_from(acl_path).unwrap();
    let reloaded_entry = reloaded_acl.get_entry(&node_id).unwrap();
    assert_eq!(reloaded_entry.role, Role::Editor);
    assert_eq!(reloaded_entry.clearance, ClearanceLevel::Secret);
    assert_eq!(
        reloaded_entry.namespaces.as_deref(),
        Some(&["workspace:test".to_string()][..])
    );
    assert_eq!(reloaded_entry.public_key_hex, "test-public-key");

    // Remove entry
    acl.remove_entry(&node_id).unwrap();
    assert!(acl.get_entry(&node_id).is_none());
}
