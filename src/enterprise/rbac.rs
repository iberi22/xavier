//! Role-Based Access Control (RBAC)
//!
//! Defines permissions, roles, and permission checking logic.

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

/// Permission types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Read,
    Write,
    Delete,
    Share,
    Manage,
}

impl Permission {
    /// Get all permissions
    pub fn all() -> Vec<Permission> {
        vec![
            Permission::Read,
            Permission::Write,
            Permission::Delete,
            Permission::Share,
            Permission::Manage,
        ]
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Permission::Read => write!(f, "read"),
            Permission::Write => write!(f, "write"),
            Permission::Delete => write!(f, "delete"),
            Permission::Share => write!(f, "share"),
            Permission::Manage => write!(f, "manage"),
        }
    }
}

/// Role types with associated permissions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Editor,
    Viewer,
    Custom(String),
}

impl Role {
    /// Check if this role has a permission
    pub fn has_permission(&self, perm: &Permission) -> bool {
        match self {
            Role::Admin => true,
            Role::Editor => matches!(
                perm,
                Permission::Read | Permission::Write | Permission::Share
            ),
            Role::Viewer => matches!(perm, Permission::Read),
            Role::Custom(_) => false, // Custom role logic would be implemented here
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::Admin => write!(f, "admin"),
            Role::Editor => write!(f, "editor"),
            Role::Viewer => write!(f, "viewer"),
            Role::Custom(s) => write!(f, "custom({})", s),
        }
    }
}

/// Role assignment for a user in a workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub user_id: Uuid,
    pub role: Role,
    pub workspace_id: Uuid,
}

/// RBAC error types
#[derive(Error, Debug)]
pub enum RbacError {
    #[error("Permission denied: {0}")]
    PermissionDenied(Permission),
    #[error("Invalid role for operation")]
    InvalidRole,
    #[error("User not found: {0}")]
    UserNotFound(Uuid),
}

/// Authorization check
pub fn authorize(user_id: Uuid, action: Permission, _resource: String) -> Result<(), RbacError> {
    // Scaffolding: In a real implementation, this would look up the user's role
    // for the relevant workspace/resource. For now, we assume success for scaffolding.
    tracing::debug!(?user_id, ?action, "Authorizing action on resource");
    Ok(())
}

/// User entity with role
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub role: Role,
    pub email: Option<String>,
    pub name: Option<String>,
    pub active: bool,
}

impl User {
    /// New.
    pub fn new(tenant_id: Uuid, role: Role) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id,
            role,
            email: None,
            name: None,
            active: true,
        }
    }

    /// With email.
    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// With name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// Check permission result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionCheck {
    pub allowed: bool,
    pub permission: Permission,
    pub user_id: Option<Uuid>,
    pub reason: Option<String>,
}

impl PermissionCheck {
    /// Allow.
    pub fn allow(permission: Permission, user_id: Uuid) -> Self {
        Self {
            allowed: true,
            permission,
            user_id: Some(user_id),
            reason: None,
        }
    }

    /// Deny.
    pub fn deny(permission: Permission, user_id: Uuid, reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            permission,
            user_id: Some(user_id),
            reason: Some(reason.into()),
        }
    }
}

/// Role guard for checking permissions
pub struct RoleGuard {
    role: Role,
    user_id: Uuid,
}

impl RoleGuard {
    /// New.
    pub fn new(role: Role, user_id: Uuid) -> Self {
        Self { role, user_id }
    }

    /// Check if role has permission
    pub fn can(&self, permission: &Permission) -> bool {
        self.role.has_permission(permission)
    }

    /// Require permission or return error
    pub fn require(&self, permission: Permission) -> Result<(), RbacError> {
        if self.can(&permission) {
            Ok(())
        } else {
            Err(RbacError::PermissionDenied(permission))
        }
    }

    /// Check and return detailed result
    pub fn check(&self, permission: Permission) -> PermissionCheck {
        if self.can(&permission) {
            PermissionCheck::allow(permission, self.user_id)
        } else {
            PermissionCheck::deny(permission, self.user_id, "Role does not have permission")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_permissions() {
        assert!(Role::Admin.has_permission(&Permission::Delete));
        assert!(Role::Editor.has_permission(&Permission::Read));
        assert!(Role::Editor.has_permission(&Permission::Write));
        assert!(!Role::Editor.has_permission(&Permission::Delete));

        assert!(Role::Viewer.has_permission(&Permission::Read));
        assert!(!Role::Viewer.has_permission(&Permission::Write));
    }

    #[test]
    fn test_role_guard() {
        let guard = RoleGuard::new(Role::Editor, Uuid::new_v4());

        assert!(guard.can(&Permission::Read));
        assert!(guard.can(&Permission::Write));
        assert!(!guard.can(&Permission::Delete));

        assert!(guard.require(Permission::Read).is_ok());
        assert!(guard.require(Permission::Delete).is_err());
    }
}
