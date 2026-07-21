// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Audit logging for compliance and security
//!
//! Tracks all operations with timestamps, tenant context, and action details.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use thiserror::Error;
use uuid::Uuid;

use crate::enterprise::tenant::TenantId;

/// Audit action types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditAction {
    MemorySearch,
    MemoryAdd,
    MemoryUpdate,
    MemoryDelete,
    MemoryGet,
    TenantCreate,
    TenantUpdate,
    TenantDelete,
    ApiKeyCreate,
    ApiKeyRevoke,
    RateLimitExceeded,
    PermissionDenied,
    Login,
    Logout,
    Other(String),
}

impl AuditAction {
    pub fn as_str(&self) -> &str {
        match self {
            AuditAction::MemorySearch => "memory.search",
            AuditAction::MemoryAdd => "memory.add",
            AuditAction::MemoryUpdate => "memory.update",
            AuditAction::MemoryDelete => "memory.delete",
            AuditAction::MemoryGet => "memory.get",
            AuditAction::TenantCreate => "tenant.create",
            AuditAction::TenantUpdate => "tenant.update",
            AuditAction::TenantDelete => "tenant.delete",
            AuditAction::ApiKeyCreate => "api_key.create",
            AuditAction::ApiKeyRevoke => "api_key.revoke",
            AuditAction::RateLimitExceeded => "rate_limit.exceeded",
            AuditAction::PermissionDenied => "permission.denied",
            AuditAction::Login => "auth.login",
            AuditAction::Logout => "auth.logout",
            AuditAction::Other(s) => s,
        }
    }
}

/// Audit entry representing a single logged event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub tenant_id: TenantId,
    pub user_id: Option<Uuid>,
    pub action: AuditAction,
    pub resource: String,
    pub resource_id: Option<String>,
    pub success: bool,
    pub details: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

impl AuditEntry {
    pub fn new(tenant_id: TenantId, action: AuditAction, resource: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            tenant_id,
            user_id: None,
            action,
            resource: resource.into(),
            resource_id: None,
            success: true,
            details: None,
            ip_address: None,
            user_agent: None,
        }
    }

    pub fn with_user(mut self, user_id: Uuid) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_resource_id(mut self, id: impl Into<String>) -> Self {
        self.resource_id = Some(id.into());
        self
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn with_ip(mut self, ip: impl Into<String>) -> Self {
        self.ip_address = Some(ip.into());
        self
    }

    pub fn with_user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    pub fn failed(mut self) -> Self {
        self.success = false;
        self
    }
}

/// Audit log store with retention policy
pub struct AuditLog {
    entries: VecDeque<AuditEntry>,
    max_entries: usize,
}

impl AuditLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries),
            max_entries,
        }
    }

    /// Log an audit entry
    pub fn log(&mut self, entry: AuditEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Log an audit event
    pub fn record(
        &mut self,
        tenant_id: TenantId,
        action: AuditAction,
        resource: impl Into<String>,
    ) -> &AuditEntry {
        let entry = AuditEntry::new(tenant_id, action, resource);
        self.log(entry);
        self.entries
            .back()
            .expect("audit_log: record called on empty log after push")
    }

    /// Get entries for a tenant
    pub fn get_for_tenant(&self, tenant_id: &TenantId) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .filter(|e| e.tenant_id == *tenant_id)
            .collect()
    }

    /// Get entries for a user
    pub fn get_for_user(&self, user_id: &Uuid) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .filter(|e| e.user_id == Some(*user_id))
            .collect()
    }

    /// Get entries by action type
    pub fn get_by_action(&self, tenant_id: &TenantId, action: &AuditAction) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .filter(|e| e.tenant_id == *tenant_id && e.action.as_str() == action.as_str())
            .collect()
    }

    /// Get entries within time range
    pub fn get_in_range(
        &self,
        tenant_id: &TenantId,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .filter(|e| e.tenant_id == *tenant_id && e.timestamp >= start && e.timestamp <= end)
            .collect()
    }

    /// Get recent entries for tenant
    pub fn get_recent(&self, tenant_id: &TenantId, limit: usize) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .rev()
            .filter(|e| e.tenant_id == *tenant_id)
            .take(limit)
            .collect()
    }

    /// Get all entries (admin only)
    pub fn get_all(&self) -> Vec<&AuditEntry> {
        self.entries.iter().collect()
    }

    /// Get entry count
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries: 100_000,
        }
    }
}

/// Audit error types
#[derive(Error, Debug)]
pub enum AuditError {
    #[error("Entry not found: {0}")]
    NotFound(String),
    #[error("Access denied to audit log")]
    AccessDenied,
}

/// Audit query parameters
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditQuery {
    pub tenant_id: Option<TenantId>,
    pub user_id: Option<Uuid>,
    pub action: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl AuditQuery {
    pub fn for_tenant(tenant_id: TenantId) -> Self {
        Self {
            tenant_id: Some(tenant_id),
            ..Default::default()
        }
    }

    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }

    pub fn with_date_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.start_date = Some(start);
        self.end_date = Some(end);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_entry() {
        let tenant_id = Uuid::new_v4();
        let entry = AuditEntry::new(tenant_id, AuditAction::MemoryAdd, "memory")
            .with_user(Uuid::new_v4())
            .with_resource_id("mem-123")
            .with_details("Added new memory");

        assert_eq!(entry.tenant_id, tenant_id);
        assert!(entry.success);
    }

    #[test]
    fn test_audit_log() {
        let mut log = AuditLog::new();
        let tenant_id = Uuid::new_v4();

        log.record(tenant_id, AuditAction::MemoryAdd, "memory");
        log.record(tenant_id, AuditAction::MemorySearch, "memory");

        let entries = log.get_for_tenant(&tenant_id);
        assert_eq!(entries.len(), 2);
    }
}
