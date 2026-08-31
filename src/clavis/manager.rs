//! Clavis KeyLeaseManager + on_task_start (WAVE-3.04)
//!
//! Intercepts ModelProviderClient to auto-lend ephemeral leases when a task starts.
//! Leases are short-lived (TTL 15m default) and tied to agent_id + task_id.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::secrets::lending::{AuditLogger, DefaultAuditLogger, EphemeralLease};

/// Clavis KeyLeaseManager — auto-lends keys on task_start
pub struct KeyLeaseManager<A: AuditLogger = DefaultAuditLogger> {
    leases: RwLock<HashMap<String, EphemeralLease>>,
    audit: A,
    default_ttl_secs: u64,
}

impl<A: AuditLogger> KeyLeaseManager<A> {
    pub fn new(audit: A, default_ttl_secs: u64) -> Self {
        Self {
            leases: RwLock::new(HashMap::new()),
            audit,
            default_ttl_secs,
        }
    }

    pub fn with_default_audit(default_ttl_secs: u64) -> KeyLeaseManager<DefaultAuditLogger>
    where
        A: Default,
    {
        KeyLeaseManager {
            leases: RwLock::new(HashMap::new()),
            audit: DefaultAuditLogger,
            default_ttl_secs,
        }
    }

    /// Called on task_start — auto-lends all keys needed for the task
    pub fn on_task_start(
        &self,
        agent_id: &str,
        task_id: &str,
        required_secret_ids: &[String],
    ) -> Result<Vec<String>> {
        let mut tokens = Vec::new();
        let mut leases = self.leases.write().unwrap();
        for secret_id in required_secret_ids {
            let token = format!("lease_{}_{}_{}", agent_id, task_id, Uuid::new_v4());
            let lease = EphemeralLease {
                session_token: token.clone(),
                real_secret_id: secret_id.clone(),
                agent_id: agent_id.to_string(),
                expires_at: std::time::SystemTime::now()
                    + std::time::Duration::from_secs(self.default_ttl_secs),
            };
            self.audit
                .log_lend(agent_id, secret_id, &token, self.default_ttl_secs);
            leases.insert(token.clone(), lease);
            tokens.push(token);
        }
        Ok(tokens)
    }

    /// Revoke all leases for a task (on task completion)
    pub fn on_task_end(&self, agent_id: &str, task_id: &str) {
        let mut leases = self.leases.write().unwrap();
        let prefix = format!("lease_{}_{}_", agent_id, task_id);
        let to_revoke: Vec<String> = leases
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        for token in to_revoke {
            leases.remove(&token);
            self.audit.log_revoke(agent_id, &token, "task_end");
        }
    }

    /// Resolve a lease token to real secret id if not expired
    pub fn resolve(&self, token: &str) -> Option<String> {
        let leases = self.leases.read().unwrap();
        let lease = leases.get(token)?;
        if lease.expires_at < std::time::SystemTime::now() {
            return None;
        }
        Some(lease.real_secret_id.clone())
    }

    /// Active lease count
    pub fn lease_count(&self) -> usize {
        self.leases.read().unwrap().len()
    }

    /// Cleanup expired leases
    pub fn cleanup_expired(&self) -> usize {
        let mut leases = self.leases.write().unwrap();
        let now = std::time::SystemTime::now();
        let before = leases.len();
        leases.retain(|_, v| v.expires_at > now);
        before - leases.len()
    }

    /// Intercept ModelProviderClient request — inject lease token header if needed
    pub fn intercept_headers(&self, token: &str) -> Option<HashMap<String, String>> {
        let real = self.resolve(token)?;
        let mut headers = HashMap::new();
        headers.insert("X-Clavis-Lease".to_string(), token.to_string());
        headers.insert(
            "X-Clavis-Secret-Hint".to_string(),
            format!("{}...", &real[..4.min(real.len())]),
        );
        Some(headers)
    }
}

impl Default for KeyLeaseManager<DefaultAuditLogger> {
    fn default() -> Self {
        Self {
            leases: RwLock::new(HashMap::new()),
            audit: DefaultAuditLogger,
            default_ttl_secs: 900,
        }
    }
}

/// Global singleton for Clavis manager
static GLOBAL_MANAGER: std::sync::OnceLock<Arc<KeyLeaseManager>> = std::sync::OnceLock::new();

pub fn global_manager() -> Arc<KeyLeaseManager> {
    GLOBAL_MANAGER
        .get_or_init(|| Arc::new(KeyLeaseManager::default()))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clavis_lease_on_task_start() {
        let m = KeyLeaseManager::default();
        let tokens = m
            .on_task_start("agent1", "task42", &["openai_key".to_string()])
            .unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(m.lease_count(), 1);
        let real = m.resolve(&tokens[0]).unwrap();
        assert_eq!(real, "openai_key");
    }

    #[test]
    fn test_clavis_task_end_revokes() {
        let m = KeyLeaseManager::default();
        let _ = m
            .on_task_start("agent1", "task1", &["k1".into(), "k2".into()])
            .unwrap();
        assert_eq!(m.lease_count(), 2);
        m.on_task_end("agent1", "task1");
        assert_eq!(m.lease_count(), 0);
    }

    #[test]
    fn test_clavis_intercept_headers() {
        let m = KeyLeaseManager::default();
        let tokens = m
            .on_task_start("agent1", "task99", &["secret_xyz".into()])
            .unwrap();
        let headers = m.intercept_headers(&tokens[0]).unwrap();
        assert!(headers.contains_key("X-Clavis-Lease"));
    }

    #[test]
    fn test_clavis_cleanup_expired() {
        let m = KeyLeaseManager::new(DefaultAuditLogger, 0);
        let _ = m.on_task_start("a", "t", &["k".into()]).unwrap();
        // ttl 0 → immediately expired
        std::thread::sleep(std::time::Duration::from_millis(10));
        let cleaned = m.cleanup_expired();
        assert!(cleaned >= 1);
    }
}
