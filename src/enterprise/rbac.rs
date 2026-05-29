//! Role-Based Access Control (RBAC)
//!
//! Defines permissions, roles, and permission checking logic.

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Permission types for memory operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    MemoryRead,
    MemoryWrite,
    MemoryDelete,
    TenantManage,
    ApiKeyManage,
    AuditView,
    Admin,
}

impl Permission {
    /// Get all permissions for a role
    pub fn all() -> Vec<Permission> {
        vec![
            Permission::MemoryRead,
            Permission::MemoryWrite,
            Permission::MemoryDelete,
            Permission::TenantManage,
            Permission::ApiKeyManage,
            Permission::AuditView,
            Permission::Admin,
        ]
    }

    /// Check if this permission implies another
    pub fn implies(&self, other: &Permission) -> bool {
        match (self, other) {
            (Permission::Admin, _) => true,
            (_, Permission::Admin) => false,
            _ => self == other,
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Permission::MemoryRead => write!(f, "memory:read"),
            Permission::MemoryWrite => write!(f, "memory:write"),
            Permission::MemoryDelete => write!(f, "memory:delete"),
            Permission::TenantManage => write!(f, "tenant:manage"),
            Permission::ApiKeyManage => write!(f, "api_key:manage"),
            Permission::AuditView => write!(f, "audit:view"),
            Permission::Admin => write!(f, "admin"),
        }
    }
}

/// Role types with associated permissions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Admin,
    Editor,
    Reader,
}

impl Role {
    /// Get permissions for this role
    pub fn permissions(&self) -> Vec<Permission> {
        match self {
            Role::Admin => Permission::all(),
            Role::Editor => vec![
                Permission::MemoryRead,
                Permission::MemoryWrite,
                Permission::AuditView,
            ],
            Role::Reader => vec![Permission::MemoryRead],
        }
    }

    /// Check if this role has a permission
    pub fn has_permission(&self, perm: &Permission) -> bool {
        match self {
            Role::Admin => true,
            Role::Editor => matches!(
                perm,
                Permission::MemoryRead | Permission::MemoryWrite | Permission::AuditView
            ),
            Role::Reader => *perm == Permission::MemoryRead,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::Admin => write!(f, "admin"),
            Role::Editor => write!(f, "editor"),
            Role::Reader => write!(f, "reader"),
        }
    }
}

/// User entity with role
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: uuid::Uuid,
    pub tenant_id: uuid::Uuid,
    pub role: Role,
    pub email: Option<String>,
    pub name: Option<String>,
    pub active: bool,
}

impl User {
    pub fn new(tenant_id: uuid::Uuid, role: Role) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            tenant_id,
            role,
            email: None,
            name: None,
            active: true,
        }
    }

    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// RBAC error types
#[derive(Error, Debug)]
pub enum RbacError {
    #[error("Permission denied: {0}")]
    PermissionDenied(Permission),
    #[error("Invalid role for operation")]
    InvalidRole,
    #[error("User not found: {0}")]
    UserNotFound(uuid::Uuid),
}

/// Check permission result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionCheck {
    pub allowed: bool,
    pub permission: Permission,
    pub user_id: Option<uuid::Uuid>,
    pub reason: Option<String>,
}

impl PermissionCheck {
    pub fn allow(permission: Permission, user_id: uuid::Uuid) -> Self {
        Self {
            allowed: true,
            permission,
            user_id: Some(user_id),
            reason: None,
        }
    }

    pub fn deny(permission: Permission, user_id: uuid::Uuid, reason: impl Into<String>) -> Self {
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
    user_id: uuid::Uuid,
}

impl RoleGuard {
    pub fn new(role: Role, user_id: uuid::Uuid) -> Self {
        Self { role, user_id }
    }

    /// Check if role has permission
    pub fn can(&self, permission: Permission) -> bool {
        self.role.has_permission(&permission)
    }

    /// Require permission or return error
    pub fn require(&self, permission: Permission) -> Result<(), RbacError> {
        if self.can(permission) {
            Ok(())
        } else {
            Err(RbacError::PermissionDenied(permission))
        }
    }

    /// Check and return detailed result
    pub fn check(&self, permission: Permission) -> PermissionCheck {
        if self.can(permission) {
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
        assert!(Role::Admin.has_permission(&Permission::Admin));
        assert!(Role::Admin.has_permission(&Permission::MemoryRead));
        assert!(Role::Admin.has_permission(&Permission::MemoryWrite));
        assert!(Role::Admin.has_permission(&Permission::MemoryDelete));

        assert!(Role::Editor.has_permission(&Permission::MemoryRead));
        assert!(Role::Editor.has_permission(&Permission::MemoryWrite));
        assert!(!Role::Editor.has_permission(&Permission::MemoryDelete));

        assert!(Role::Reader.has_permission(&Permission::MemoryRead));
        assert!(!Role::Reader.has_permission(&Permission::MemoryWrite));
    }

    #[test]
    fn test_role_guard() {
        let guard = RoleGuard::new(Role::Editor, uuid::Uuid::new_v4());

        assert!(guard.can(Permission::MemoryRead));
        assert!(guard.can(Permission::MemoryWrite));
        assert!(!guard.can(Permission::MemoryDelete));

        assert!(guard.require(Permission::MemoryRead).is_ok());
        assert!(guard.require(Permission::MemoryDelete).is_err());
    }
}
