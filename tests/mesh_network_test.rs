//! WX-205 — MeshNetwork unit tests (crear red, añadir miembro, grant cruzado, revocar, check_permission con y sin expiración)

use chrono::{Duration, Utc};
use xavier::enterprise::rbac::Permission;
use xavier::mesh::network::MeshNetwork;

fn net(id: &str, owner: &str) -> MeshNetwork {
    MeshNetwork::create_network(id.to_string(), format!("Network {}", id), owner.to_string())
}

#[test]
fn test_crear_red() {
    let n = net("net-1", "node-a");
    assert_eq!(n.id, "net-1");
    assert_eq!(n.owner_node, "node-a");
    assert!(n.members.contains(&"node-a".to_string()));
}

#[test]
fn test_anadir_miembro() {
    let mut n = net("net-1", "node-a");
    n.add_member("node-b".to_string()).unwrap();
    assert!(n.members.contains(&"node-b".to_string()));
    assert!(n.add_member("node-b".to_string()).is_err());
}

#[test]
fn test_grant_cruzado() {
    let mut n = net("net-1", "node-a");
    n.add_member("node-b".to_string()).unwrap();
    let g = n.grant_cross(
        "resource-1".to_string(),
        "node-b".to_string(),
        Permission::Read,
        None,
    );
    assert!(!g.revoked);
    assert!(n.check_permission("node-b", "resource-1", &Permission::Read));
    assert!(!n.check_permission("node-b", "resource-1", &Permission::Write));
}

#[test]
fn test_revocar() {
    let mut n = net("net-1", "node-a");
    let g = n.grant_cross(
        "res-1".to_string(),
        "node-b".to_string(),
        Permission::Read,
        None,
    );
    assert!(n.check_permission("node-b", "res-1", &Permission::Read));
    n.revoke_grant(&g.id).unwrap();
    assert!(!n.check_permission("node-b", "res-1", &Permission::Read));
    assert!(n.revoke_grant(&g.id).is_err());
}

#[test]
fn test_check_permission_con_expiracion() {
    let mut n = net("net-1", "node-a");
    let past = Utc::now() - Duration::hours(1);
    n.grant_cross(
        "res-1".to_string(),
        "node-b".to_string(),
        Permission::Read,
        Some(past),
    );
    assert!(!n.check_permission("node-b", "res-1", &Permission::Read));
}

#[test]
fn test_check_permission_sin_expiracion() {
    let mut n = net("net-1", "node-a");
    n.grant_cross(
        "res-1".to_string(),
        "node-b".to_string(),
        Permission::Write,
        None,
    );
    assert!(n.check_permission("node-b", "res-1", &Permission::Write));
    assert!(n.acl.grants[0].is_active());
}

#[test]
fn test_check_permission_futura_activa() {
    let mut n = net("net-2", "node-a");
    let future = Utc::now() + Duration::hours(1);
    n.grant_cross(
        "res-1".to_string(),
        "node-b".to_string(),
        Permission::Read,
        Some(future),
    );
    assert!(n.check_permission("node-b", "res-1", &Permission::Read));
}

#[test]
fn test_owner_tiene_todos_permisos() {
    let n = net("net-1", "node-a");
    assert!(n.check_permission("node-a", "any", &Permission::Manage));
}

#[test]
fn test_registry_nodo_en_n_redes() {
    use xavier::mesh::network::MeshNetworkRegistry;
    let mut reg = MeshNetworkRegistry::new();
    reg.create(
        "net-1".to_string(),
        "Net 1".to_string(),
        "node-a".to_string(),
    )
    .unwrap();
    reg.create(
        "net-2".to_string(),
        "Net 2".to_string(),
        "node-b".to_string(),
    )
    .unwrap();
    reg.get_mut("net-2")
        .unwrap()
        .add_member("node-a".to_string())
        .unwrap();
    let nets = reg.list_for_node("node-a");
    assert_eq!(nets.len(), 2);
}

#[test]
fn test_private_mesh_registry_delegacion() {
    use tempfile::NamedTempFile;
    use xavier::mesh::PrivateMeshRegistry;
    let file = NamedTempFile::new().unwrap();
    let mut reg = PrivateMeshRegistry::load_or_create(file.path().to_path_buf()).unwrap();
    let net = reg
        .create_network(
            "net-1".to_string(),
            "Net 1".to_string(),
            "node-a".to_string(),
        )
        .unwrap();
    assert_eq!(net.id, "net-1");
    reg.add_member("net-1", "node-b".to_string()).unwrap();
    let grant = reg
        .grant_cross(
            "net-1",
            "doc-1".to_string(),
            "node-b".to_string(),
            Permission::Read,
            None,
        )
        .unwrap();
    assert!(reg.check_permission("node-b", "doc-1"));
    assert!(reg.check_permission_with_perm("node-b", "doc-1", &Permission::Read));
    assert!(!reg.check_permission_with_perm("node-b", "doc-1", &Permission::Write));
    reg.revoke_grant("net-1", &grant.id).unwrap();
    assert!(!reg.check_permission("node-b", "doc-1"));
}
