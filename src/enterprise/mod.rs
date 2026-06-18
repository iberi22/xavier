//! Enterprise features module
//!
//! Provides:
//! - Multi-tenancy
//! - Role-based access control (RBAC)
//! - Audit logging
//! - API key management
//! - Rate limiting

pub mod audit;
#[cfg(feature = "enterprise")]
pub mod http;
pub mod keys;
pub mod rate_limit;
pub mod rbac;
pub mod tenant;
#[cfg(not(feature = "enterprise"))]
pub mod http {
    pub fn enterprise_router() -> axum::Router {
        axum::Router::new()
    }
}
pub mod persistence;
#[cfg(test)]
pub mod tests;

pub use audit::{AuditAction, AuditEntry, AuditLog};
pub use keys::{ApiKey, ApiKeyStore, ApiKeyType};
pub use rate_limit::{RateLimitConfig, RateLimitKey, RateLimitResult, RateLimiter};
pub use rbac::{Permission, PermissionCheck, Role, RoleGuard};
pub use tenant::{Plan, Tenant, TenantId, TenantStore, Workspace};
