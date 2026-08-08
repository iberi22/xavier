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

// ─── Clearance Levels ──────────────────────────────────────────────────────

/// Information classification levels (inspired by government/enterprise models).
/// Higher variants subsume lower ones: TopSecret > Secret > Confidential > Internal > Public.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClearanceLevel {
    Public = 0,
    Internal = 1,
    Confidential = 2,
    Secret = 3,
    TopSecret = 4,
}

impl std::fmt::Display for ClearanceLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClearanceLevel::Public => write!(f, "public"),
            ClearanceLevel::Internal => write!(f, "internal"),
            ClearanceLevel::Confidential => write!(f, "confidential"),
            ClearanceLevel::Secret => write!(f, "secret"),
            ClearanceLevel::TopSecret => write!(f, "top_secret"),
        }
    }
}

impl ClearanceLevel {
    /// Parse from string, case-insensitive.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "public" => Some(ClearanceLevel::Public),
            "internal" => Some(ClearanceLevel::Internal),
            "confidential" => Some(ClearanceLevel::Confidential),
            "secret" => Some(ClearanceLevel::Secret),
            "top_secret" | "topsecret" | "top secret" => Some(ClearanceLevel::TopSecret),
            _ => None,
        }
    }
}

/// Maps roles to their maximum clearance level and checks document access.
#[derive(Debug, Clone)]
pub struct ClearanceManager {
    role_clearance: HashMap<AclRole, ClearanceLevel>,
}

impl Default for ClearanceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ClearanceManager {
    /// Default mapping: Viewer=Public, Colaborador=Confidential, Admin=TopSecret.
    pub fn new() -> Self {
        let mut role_clearance = HashMap::new();
        role_clearance.insert(AclRole::Viewer, ClearanceLevel::Public);
        role_clearance.insert(AclRole::Colaborador, ClearanceLevel::Confidential);
        role_clearance.insert(AclRole::Admin, ClearanceLevel::TopSecret);
        Self { role_clearance }
    }

    /// Set the clearance level for a role.
    pub fn set_clearance(&mut self, role: AclRole, level: ClearanceLevel) {
        self.role_clearance.insert(role, level);
    }

    /// Get the clearance level for a role.
    pub fn get_clearance(&self, role: &AclRole) -> ClearanceLevel {
        self.role_clearance
            .get(role)
            .copied()
            .unwrap_or(ClearanceLevel::Public)
    }

    /// Check if a role can access a document at the given classification level.
    /// Access is granted when the role's clearance >= document classification.
    pub fn can_access(&self, role: AclRole, document_level: ClearanceLevel) -> bool {
        self.get_clearance(&role) >= document_level
    }
}

// ─── Groups & Permissions ──────────────────────────────────────────────────

/// A named group of users with attached permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub permissions: HashSet<AclPermission>,
    /// Maximum clearance level this group can grant.
    pub max_clearance: ClearanceLevel,
}

/// Manages groups: membership, permission grants, and access checks.
#[derive(Debug, Clone)]
pub struct GroupManager {
    groups: HashMap<String, Group>,
    /// user_id -> set of group names
    memberships: HashMap<String, HashSet<String>>,
}

impl Default for GroupManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GroupManager {
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
            memberships: HashMap::new(),
        }
    }

    /// Create a new group.
    pub fn create_group(
        &mut self,
        name: String,
        permissions: HashSet<AclPermission>,
        max_clearance: ClearanceLevel,
    ) {
        self.groups.insert(
            name.clone(),
            Group {
                name,
                permissions,
                max_clearance,
            },
        );
    }

    /// Remove a group. Returns true if it existed.
    pub fn remove_group(&mut self, name: &str) -> bool {
        self.groups.remove(name).is_some()
    }

    /// Add a user to a group. Returns false if the group doesn't exist.
    pub fn add_member(&mut self, user_id: &str, group_name: &str) -> bool {
        if !self.groups.contains_key(group_name) {
            return false;
        }
        self.memberships
            .entry(user_id.to_string())
            .or_default()
            .insert(group_name.to_string());
        true
    }

    /// Remove a user from a group. Returns true if the membership existed.
    pub fn remove_member(&mut self, user_id: &str, group_name: &str) -> bool {
        self.memberships
            .get_mut(user_id)
            .map(|groups| groups.remove(group_name))
            .unwrap_or(false)
    }

    /// Get all groups a user belongs to.
    pub fn user_groups(&self, user_id: &str) -> Vec<&str> {
        self.memberships
            .get(user_id)
            .map(|gs| gs.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    /// Check if a user has a specific permission via any of their groups.
    pub fn user_has_permission(&self, user_id: &str, permission: AclPermission) -> bool {
        if let Some(groups) = self.memberships.get(user_id) {
            for gname in groups {
                if let Some(group) = self.groups.get(gname) {
                    if group.permissions.contains(&permission) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Get the effective max clearance for a user (highest across all groups).
    pub fn user_max_clearance(&self, user_id: &str) -> ClearanceLevel {
        self.memberships
            .get(user_id)
            .map(|gs| {
                gs.iter()
                    .filter_map(|gname| self.groups.get(gname).map(|g| g.max_clearance))
                    .max()
                    .unwrap_or(ClearanceLevel::Public)
            })
            .unwrap_or(ClearanceLevel::Public)
    }

    /// Check if a user can access a document at the given classification level.
    pub fn user_can_access(&self, user_id: &str, document_level: ClearanceLevel) -> bool {
        self.user_max_clearance(user_id) >= document_level
    }

    /// List all groups (name, member count, permissions).
    pub fn list_groups(&self) -> Vec<(&str, usize, &HashSet<AclPermission>)> {
        self.groups
            .values()
            .map(|g| {
                let count = self
                    .memberships
                    .values()
                    .filter(|gs| gs.contains(&g.name))
                    .count();
                (g.name.as_str(), count, &g.permissions)
            })
            .collect()
    }
}

#[cfg(test)]
mod clearance_tests {
    use super::*;

    #[test]
    fn test_clearance_ordering() {
        assert!(ClearanceLevel::Public < ClearanceLevel::Internal);
        assert!(ClearanceLevel::Internal < ClearanceLevel::Confidential);
        assert!(ClearanceLevel::Confidential < ClearanceLevel::Secret);
        assert!(ClearanceLevel::Secret < ClearanceLevel::TopSecret);
    }

    #[test]
    fn test_clearance_from_str() {
        assert_eq!(
            ClearanceLevel::from_str("public"),
            Some(ClearanceLevel::Public)
        );
        assert_eq!(
            ClearanceLevel::from_str("TOP_SECRET"),
            Some(ClearanceLevel::TopSecret)
        );
        assert_eq!(ClearanceLevel::from_str("bogus"), None);
    }

    #[test]
    fn test_clearance_manager_default_mapping() {
        let cm = ClearanceManager::new();
        assert!(cm.can_access(AclRole::Viewer, ClearanceLevel::Public));
        assert!(!cm.can_access(AclRole::Viewer, ClearanceLevel::Internal));
        assert!(cm.can_access(AclRole::Colaborador, ClearanceLevel::Confidential));
        assert!(!cm.can_access(AclRole::Colaborador, ClearanceLevel::Secret));
        assert!(cm.can_access(AclRole::Admin, ClearanceLevel::TopSecret));
    }

    #[test]
    fn test_clearance_manager_custom() {
        let mut cm = ClearanceManager::new();
        cm.set_clearance(AclRole::Viewer, ClearanceLevel::Internal);
        assert!(cm.can_access(AclRole::Viewer, ClearanceLevel::Internal));
        assert!(!cm.can_access(AclRole::Viewer, ClearanceLevel::Confidential));
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;

    fn test_group(_name: &str) -> (HashSet<AclPermission>, ClearanceLevel) {
        let mut perms = HashSet::new();
        perms.insert(AclPermission::Read);
        (perms, ClearanceLevel::Internal)
    }

    #[test]
    fn test_group_lifecycle() {
        let mut gm = GroupManager::new();
        let (perms, clear) = test_group("readers");
        gm.create_group("readers".to_string(), perms, clear);

        assert!(gm.add_member("alice", "readers"));
        assert!(!gm.add_member("alice", "nonexistent"));

        assert_eq!(gm.user_groups("alice"), vec!["readers"]);
        assert!(gm.user_has_permission("alice", AclPermission::Read));
        assert!(!gm.user_has_permission("alice", AclPermission::Write));

        assert!(gm.remove_member("alice", "readers"));
        assert!(!gm.user_has_permission("alice", AclPermission::Read));
    }

    #[test]
    fn test_group_max_clearance_union() {
        let mut gm = GroupManager::new();

        let mut low_perms = HashSet::new();
        low_perms.insert(AclPermission::Read);
        gm.create_group("low".to_string(), low_perms, ClearanceLevel::Internal);

        let mut high_perms = HashSet::new();
        high_perms.insert(AclPermission::Manage);
        gm.create_group("high".to_string(), high_perms, ClearanceLevel::Secret);

        gm.add_member("bob", "low");
        gm.add_member("bob", "high");

        // Effective clearance is the max across groups
        assert_eq!(gm.user_max_clearance("bob"), ClearanceLevel::Secret);
        assert!(gm.user_can_access("bob", ClearanceLevel::Secret));
        assert!(!gm.user_can_access("bob", ClearanceLevel::TopSecret));
    }

    #[test]
    fn test_group_removal() {
        let mut gm = GroupManager::new();
        let (perms, clear) = test_group("temp");
        gm.create_group("temp".to_string(), perms, clear);
        gm.add_member("carol", "temp");
        assert!(gm.remove_group("temp"));
        // Membership still exists but group is gone → permission check fails
        assert!(!gm.user_has_permission("carol", AclPermission::Read));
    }
}
