pub mod hierarchy;

pub use hierarchy::{AclRole, RoleHierarchy};

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Permissions defined for access control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AclPermission {
    Read,
    Write,
    Delete,
    Manage,
}

impl std::fmt::Display for AclPermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AclPermission::Read => write!(f, "read"),
            AclPermission::Write => write!(f, "write"),
            AclPermission::Delete => write!(f, "delete"),
            AclPermission::Manage => write!(f, "manage"),
        }
    }
}

/// Access Control List (ACL) Manager.
/// Manages the role hierarchy, default permission mappings, and permission checks.
#[derive(Debug, Clone)]
pub struct AclManager {
    pub hierarchy: RoleHierarchy,
    pub base_permissions: HashMap<AclRole, HashSet<AclPermission>>,
}

impl Default for AclManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AclManager {
    /// Creates a new AclManager with default roles and permission mappings.
    pub fn new() -> Self {
        let mut base_permissions = HashMap::new();

        // Viewer base permissions: Read
        let mut viewer_perms = HashSet::new();
        viewer_perms.insert(AclPermission::Read);
        base_permissions.insert(AclRole::Viewer, viewer_perms);

        // Colaborador base permissions: Write
        // (will inherit Read from Viewer via RoleHierarchy)
        let mut colaborador_perms = HashSet::new();
        colaborador_perms.insert(AclPermission::Write);
        base_permissions.insert(AclRole::Colaborador, colaborador_perms);

        // Admin base permissions: Delete, Manage
        // (will inherit Write and Read from Colaborador and Viewer via RoleHierarchy)
        let mut admin_perms = HashSet::new();
        admin_perms.insert(AclPermission::Delete);
        admin_perms.insert(AclPermission::Manage);
        base_permissions.insert(AclRole::Admin, admin_perms);

        Self {
            hierarchy: RoleHierarchy::new(),
            base_permissions,
        }
    }

    /// Checks if a role has the specified permission, considering inheritance.
    /// If role R1 inherits role R2, R1 will have all permissions granted to R2.
    pub fn has_permission(&self, role: AclRole, permission: AclPermission) -> bool {
        // Iterate over all roles we have mapped permissions for
        for (&mapped_role, permissions) in &self.base_permissions {
            // If the user's role inherits from the mapped role, they get its permissions
            if self.hierarchy.inherits(role, mapped_role) && permissions.contains(&permission) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acl_manager_permission_inheritance() {
        let manager = AclManager::new();

        // Viewer permissions
        assert!(manager.has_permission(AclRole::Viewer, AclPermission::Read));
        assert!(!manager.has_permission(AclRole::Viewer, AclPermission::Write));
        assert!(!manager.has_permission(AclRole::Viewer, AclPermission::Delete));
        assert!(!manager.has_permission(AclRole::Viewer, AclPermission::Manage));

        // Colaborador permissions (Write + inherited Read)
        assert!(manager.has_permission(AclRole::Colaborador, AclPermission::Read));
        assert!(manager.has_permission(AclRole::Colaborador, AclPermission::Write));
        assert!(!manager.has_permission(AclRole::Colaborador, AclPermission::Delete));
        assert!(!manager.has_permission(AclRole::Colaborador, AclPermission::Manage));

        // Admin permissions (Delete, Manage + inherited Write, Read)
        assert!(manager.has_permission(AclRole::Admin, AclPermission::Read));
        assert!(manager.has_permission(AclRole::Admin, AclPermission::Write));
        assert!(manager.has_permission(AclRole::Admin, AclPermission::Delete));
        assert!(manager.has_permission(AclRole::Admin, AclPermission::Manage));
    }
}
