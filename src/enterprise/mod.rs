//! Enterprise features module
//!
//! Provides:
//! - Multi-tenancy
//! - Role-based access control (RBAC)
//! - Audit logging
//! - API key management
//! - Rate limiting

pub mod tenancy;
pub mod rbac;
pub mod audit;
pub mod keys;
pub mod rate_limit;

pub use tenancy::{Tenant, TenantId, Plan, TenantStore};
pub use rbac::{Permission, Role, RoleGuard, PermissionCheck};
pub use audit::{AuditEntry, AuditLog, AuditAction};
pub use keys::{ApiKey, ApiKeyStore};
pub use rate_limit::{RateLimiter, RateLimitConfig, RateLimitResult, RateLimitKey};
