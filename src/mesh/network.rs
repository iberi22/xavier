//! First-class Mesh Networks — Private networks with cross-grants
//!
//! A node may belong to N distinct MeshNetworks, each with its own identity
//! and NetworkAcl. Cross-network access uses CrossGrant (resource → node →
//! Permission → expiry → revocation).

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::enterprise::rbac::Permission;

// ---------------------------------------------------------------------------
// CrossGrant
// ---------------------------------------------------------------------------

/// Granular cross-network permission grant: resource → target node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrossGrant {
    /// Unique grant identifier (ULID-like).
    pub id: String,
    /// Resource identifier the grant applies to (e.g., "memory:abc", "snapshot:xyz").
    pub resource_id: String,
    /// Node that receives the permission.
    pub target_node: String,
    /// Permission level granted.
    pub permission: Permission,
    /// Optional expiry; None means no expiry.
    pub expires_at: Option<DateTime<Utc>>,
    /// Whether the grant has been revoked.
    #[serde(default)]
    pub revoked: bool,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

impl CrossGrant {
    /// Returns true if the grant is currently active (not revoked, not expired).
    pub fn is_active(&self) -> bool {
        if self.revoked {
            return false;
        }
        if let Some(exp) = self.expires_at {
            if Utc::now() > exp {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// NetworkAcl
// ---------------------------------------------------------------------------

/// ACL for a MeshNetwork: default permission + explicit cross-grants.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NetworkAcl {
    /// Default permission for members without explicit grants.
    #[serde(default)]
    pub default_permission: Option<Permission>,
    /// Explicit cross-grants.
    #[serde(default)]
    pub grants: Vec<CrossGrant>,
}

impl NetworkAcl {
    /// Check if a node has permission for a resource via grants.
    pub fn has_permission(&self, node: &str, resource: &str, required: &Permission) -> bool {
        for g in &self.grants {
            if g.target_node == node
                && g.resource_id == resource
                && g.permission == *required
                && g.is_active()
            {
                return true;
            }
            // Wildcard resource_id "*" grants permission on any resource
            if g.target_node == node
                && g.resource_id == "*"
                && g.permission == *required
                && g.is_active()
            {
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// MeshNetwork
// ---------------------------------------------------------------------------

/// A first-class private mesh network. Nodes may belong to N networks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MeshNetwork {
    /// Unique network id (e.g., "net-001").
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Owner node id.
    pub owner_node: String,
    /// Member node ids.
    #[serde(default)]
    pub members: Vec<String>,
    /// Network ACL with cross-grants.
    #[serde(default)]
    pub acl: NetworkAcl,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl MeshNetwork {
    /// Create a new network with owner as first member.
    pub fn create_network(id: String, name: String, owner_node: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            name,
            owner_node: owner_node.clone(),
            members: vec![owner_node],
            acl: NetworkAcl::default(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Add a member node to the network.
    pub fn add_member(&mut self, node_id: String) -> Result<()> {
        if self.members.contains(&node_id) {
            return Err(anyhow!(
                "Node '{}' is already a member of network '{}'",
                node_id,
                self.id
            ));
        }
        self.members.push(node_id);
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Remove a member node from the network.
    pub fn remove_member(&mut self, node_id: &str) -> Result<()> {
        if node_id == self.owner_node {
            return Err(anyhow!(
                "Cannot remove owner node '{}' from network '{}'",
                node_id,
                self.id
            ));
        }
        let pos = self
            .members
            .iter()
            .position(|m| m == node_id)
            .ok_or_else(|| {
                anyhow!(
                    "Node '{}' is not a member of network '{}'",
                    node_id,
                    self.id
                )
            })?;
        self.members.remove(pos);
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Create a cross-grant for a target node on a resource.
    pub fn grant_cross(
        &mut self,
        resource_id: String,
        target_node: String,
        permission: Permission,
        expires_at: Option<DateTime<Utc>>,
    ) -> CrossGrant {
        let grant = CrossGrant {
            id: ulid::Ulid::new().to_string(),
            resource_id,
            target_node,
            permission,
            expires_at,
            revoked: false,
            created_at: Utc::now(),
        };
        self.acl.grants.push(grant.clone());
        self.updated_at = Utc::now();
        grant
    }

    /// Revoke a grant by id.
    pub fn revoke_grant(&mut self, grant_id: &str) -> Result<()> {
        let grant = self
            .acl
            .grants
            .iter_mut()
            .find(|g| g.id == grant_id)
            .ok_or_else(|| anyhow!("Grant '{}' not found in network '{}'", grant_id, self.id))?;
        if grant.revoked {
            return Err(anyhow!("Grant '{}' is already revoked", grant_id));
        }
        grant.revoked = true;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Check if a node has permission for a resource in this network.
    /// Returns true if node is owner, or has an active grant matching permission.
    pub fn check_permission(&self, node: &str, resource: &str, permission: &Permission) -> bool {
        // Owner has all permissions.
        if node == self.owner_node {
            return true;
        }
        // Must be a member to have any access (unless grant targets non-member — still allow via grant)
        // Check active grants first.
        if self.acl.has_permission(node, resource, permission) {
            return true;
        }
        // Fall back to default_permission if set and node is a member.
        if self.members.contains(&node.to_string()) {
            if let Some(default) = &self.acl.default_permission {
                if default == permission {
                    return true;
                }
            }
        }
        false
    }

    /// Simplified check_permission with Read as default required (bool convenience).
    pub fn check_permission_bool(&self, node: &str, resource: &str) -> bool {
        self.check_permission(node, resource, &Permission::Read)
    }
}

// ---------------------------------------------------------------------------
// MeshNetworkRegistry — in-memory collection helper
// ---------------------------------------------------------------------------

/// In-memory registry of MeshNetworks (used by PrivateMeshRegistry).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MeshNetworkRegistry {
    pub networks: HashMap<String, MeshNetwork>,
}

impl MeshNetworkRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&mut self, id: String, name: String, owner_node: String) -> Result<MeshNetwork> {
        if self.networks.contains_key(&id) {
            return Err(anyhow!("Network '{}' already exists", id));
        }
        let net = MeshNetwork::create_network(id.clone(), name, owner_node);
        self.networks.insert(id, net.clone());
        Ok(net)
    }

    pub fn get(&self, id: &str) -> Option<&MeshNetwork> {
        self.networks.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut MeshNetwork> {
        self.networks.get_mut(id)
    }

    pub fn list_for_node(&self, node: &str) -> Vec<MeshNetwork> {
        self.networks
            .values()
            .filter(|n| n.members.contains(&node.to_string()) || n.owner_node == node)
            .cloned()
            .collect()
    }

    pub fn all(&self) -> Vec<MeshNetwork> {
        self.networks.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn net(id: &str, owner: &str) -> MeshNetwork {
        MeshNetwork::create_network(id.to_string(), format!("Network {}", id), owner.to_string())
    }

    #[test]
    fn test_create_network() {
        let n = net("net-1", "node-a");
        assert_eq!(n.id, "net-1");
        assert_eq!(n.owner_node, "node-a");
        assert!(n.members.contains(&"node-a".to_string()));
        assert_eq!(n.members.len(), 1);
    }

    #[test]
    fn test_add_member() {
        let mut n = net("net-1", "node-a");
        n.add_member("node-b".to_string()).unwrap();
        assert!(n.members.contains(&"node-b".to_string()));
        // duplicate fails
        assert!(n.add_member("node-b".to_string()).is_err());
    }

    #[test]
    fn test_remove_member() {
        let mut n = net("net-1", "node-a");
        n.add_member("node-b".to_string()).unwrap();
        n.remove_member("node-b").unwrap();
        assert!(!n.members.contains(&"node-b".to_string()));
        // remove non-member fails
        assert!(n.remove_member("node-b").is_err());
        // cannot remove owner
        assert!(n.remove_member("node-a").is_err());
    }

    #[test]
    fn test_grant_cross_and_check_permission() {
        let mut n = net("net-1", "node-a");
        n.add_member("node-b".to_string()).unwrap();
        let grant = n.grant_cross(
            "resource-1".to_string(),
            "node-b".to_string(),
            Permission::Read,
            None,
        );
        assert!(!grant.revoked);
        assert!(n.check_permission("node-b", "resource-1", &Permission::Read));
        assert!(!n.check_permission("node-b", "resource-1", &Permission::Write));
        assert!(!n.check_permission("node-c", "resource-1", &Permission::Read));
    }

    #[test]
    fn test_revoke_grant() {
        let mut n = net("net-1", "node-a");
        let grant = n.grant_cross(
            "res-1".to_string(),
            "node-b".to_string(),
            Permission::Read,
            None,
        );
        assert!(n.check_permission("node-b", "res-1", &Permission::Read));
        n.revoke_grant(&grant.id).unwrap();
        assert!(!n.check_permission("node-b", "res-1", &Permission::Read));
        // double revoke fails
        assert!(n.revoke_grant(&grant.id).is_err());
        // unknown grant fails
        assert!(n.revoke_grant("unknown-id").is_err());
    }

    #[test]
    fn test_check_permission_expiry() {
        let mut n = net("net-1", "node-a");
        // expired grant
        let past = Utc::now() - Duration::hours(1);
        n.grant_cross(
            "res-1".to_string(),
            "node-b".to_string(),
            Permission::Read,
            Some(past),
        );
        assert!(!n.check_permission("node-b", "res-1", &Permission::Read));

        // future grant still active
        let future = Utc::now() + Duration::hours(1);
        let mut n2 = net("net-2", "node-a");
        n2.grant_cross(
            "res-1".to_string(),
            "node-b".to_string(),
            Permission::Read,
            Some(future),
        );
        assert!(n2.check_permission("node-b", "res-1", &Permission::Read));
    }

    #[test]
    fn test_check_permission_no_expiry() {
        let mut n = net("net-1", "node-a");
        n.grant_cross(
            "res-1".to_string(),
            "node-b".to_string(),
            Permission::Write,
            None,
        );
        assert!(n.check_permission("node-b", "res-1", &Permission::Write));
        // grant with no expiry stays active indefinitely
        assert!(n.acl.grants[0].is_active());
    }

    #[test]
    fn test_owner_has_all_permissions() {
        let n = net("net-1", "node-a");
        assert!(n.check_permission("node-a", "any-resource", &Permission::Read));
        assert!(n.check_permission("node-a", "any-resource", &Permission::Manage));
        assert!(n.check_permission("node-a", "any-resource", &Permission::Delete));
    }

    #[test]
    fn test_node_in_multiple_networks() {
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
        // node-a joins net-2
        reg.get_mut("net-2")
            .unwrap()
            .add_member("node-a".to_string())
            .unwrap();

        let nets = reg.list_for_node("node-a");
        assert_eq!(nets.len(), 2);
        let ids: Vec<&str> = nets.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"net-1"));
        assert!(ids.contains(&"net-2"));
    }

    #[test]
    fn test_grant_cross_with_expiry_future_then_revoked() {
        let mut n = net("net-1", "node-a");
        let future = Utc::now() + Duration::hours(2);
        let g = n.grant_cross(
            "doc-1".to_string(),
            "node-x".to_string(),
            Permission::Share,
            Some(future),
        );
        assert!(n.check_permission("node-x", "doc-1", &Permission::Share));
        n.revoke_grant(&g.id).unwrap();
        assert!(!n.check_permission("node-x", "doc-1", &Permission::Share));
    }

    #[test]
    fn test_default_permission() {
        let mut n = net("net-1", "node-a");
        n.add_member("node-b".to_string()).unwrap();
        n.acl.default_permission = Some(Permission::Read);
        assert!(n.check_permission("node-b", "any-res", &Permission::Read));
        assert!(!n.check_permission("node-b", "any-res", &Permission::Write));
        // non-member should not get default
        assert!(!n.check_permission("node-z", "any-res", &Permission::Read));
    }
}
