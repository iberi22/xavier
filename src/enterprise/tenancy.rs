//! Multi-tenancy support
//!
//! Manages tenant isolation, plan-based feature tiers, and tenant metadata.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

/// Tenant identifier
pub type TenantId = Uuid;

/// Subscription plan with feature tiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Plan {
    Free,
    Pro,
    Enterprise,
}

impl Plan {
    pub fn memory_limit(&self) -> usize {
        match self {
            Plan::Free => 100,
            Plan::Pro => 10_000,
            Plan::Enterprise => usize::MAX,
        }
    }

    pub fn rate_limit_rpm(&self) -> u32 {
        match self {
            Plan::Free => 30,
            Plan::Pro => 120,
            Plan::Enterprise => 1000,
        }
    }

    pub fn max_api_keys(&self) -> usize {
        match self {
            Plan::Free => 2,
            Plan::Pro => 10,
            Plan::Enterprise => usize::MAX,
        }
    }

    pub fn audit_retention_days(&self) -> u32 {
        match self {
            Plan::Free => 7,
            Plan::Pro => 90,
            Plan::Enterprise => 365,
        }
    }
}

/// Tenant entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub id: TenantId,
    pub name: String,
    pub plan: Plan,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub metadata: HashMap<String, String>,
}

impl Tenant {
    pub fn new(name: impl Into<String>, plan: Plan) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            plan,
            created_at: chrono::Utc::now(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Tenant not found error
#[derive(Error, Debug)]
pub enum TenantError {
    #[error("Tenant not found: {0}")]
    NotFound(TenantId),
    #[error("Tenant already exists: {0}")]
    AlreadyExists(TenantId),
    #[error("Invalid plan upgrade")]
    InvalidPlanUpgrade,
}

/// In-memory tenant store
pub struct TenantStore {
    tenants: HashMap<TenantId, Tenant>,
}

impl TenantStore {
    pub fn new() -> Self {
        Self {
            tenants: HashMap::new(),
        }
    }

    /// Create a new tenant
    pub fn create(&mut self, name: impl Into<String>, plan: Plan) -> Tenant {
        let tenant = Tenant::new(name, plan);
        self.tenants.insert(tenant.id, tenant.clone());
        tenant
    }

    /// Get tenant by ID
    pub fn get(&self, id: &TenantId) -> Option<&Tenant> {
        self.tenants.get(id)
    }

    /// Get mutable tenant reference
    pub fn get_mut(&mut self, id: &TenantId) -> Option<&mut Tenant> {
        self.tenants.get_mut(id)
    }

    /// List all tenants
    pub fn list(&self) -> Vec<&Tenant> {
        self.tenants.values().collect()
    }

    /// Update tenant plan
    pub fn update_plan(&mut self, id: &TenantId, plan: Plan) -> Result<(), TenantError> {
        match self.tenants.get_mut(id) {
            Some(tenant) => {
                tenant.plan = plan;
                Ok(())
            }
            None => Err(TenantError::NotFound(*id)),
        }
    }

    /// Delete tenant
    pub fn delete(&mut self, id: &TenantId) -> Result<Tenant, TenantError> {
        self.tenants
            .remove(id)
            .ok_or(TenantError::NotFound(*id))
    }

    /// Check if tenant exists
    pub fn exists(&self, id: &TenantId) -> bool {
        self.tenants.contains_key(id)
    }
}

impl Default for TenantStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_creation() {
        let store = TenantStore::new();
        let tenant = store.create("Test Tenant", Plan::Pro);
        
        assert_eq!(tenant.name, "Test Tenant");
        assert_eq!(tenant.plan, Plan::Pro);
        assert!(store.exists(&tenant.id));
    }

    #[test]
    fn test_plan_limits() {
        assert_eq!(Plan::Free.memory_limit(), 100);
        assert_eq!(Plan::Pro.memory_limit(), 10_000);
        assert_eq!(Plan::Enterprise.memory_limit(), usize::MAX);
        
        assert_eq!(Plan::Free.rate_limit_rpm(), 30);
        assert_eq!(Plan::Pro.rate_limit_rpm(), 120);
        assert_eq!(Plan::Enterprise.rate_limit_rpm(), 1000);
    }
}
