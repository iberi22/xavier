//! Secrets coordination for secure access
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub struct LeakDetector {
    /// Map of SHA-256 hashes of secrets to the agent_id they were lent to
    hashes: Arc<RwLock<HashMap<String, String>>>,
}

impl LeakDetector {
    pub fn new() -> Self {
        Self {
            hashes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_key(&self, secret_value: &str, agent_id: &str) {
        let mut hasher = Sha256::new();
        hasher.update(secret_value.as_bytes());
        let hash = crate::crypto::hex_encode(hasher.finalize());
        let mut hashes = self.hashes.write().await;
        hashes.insert(hash, agent_id.to_string());
    }

    /// Checks if any registered secret hash is present in the content.
    /// This hashes tokens in the content to see if they match.
    pub async fn check_leak(&self, content: &str) -> Option<(String, String)> {
        let hashes = self.hashes.read().await;
        if hashes.is_empty() {
            return None;
        }

        // Simple tokenization: split by common delimiters
        // In a real scenario, we might want to use a more sophisticated approach
        for token in content
            .split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ':')
        {
            if token.is_empty() {
                continue;
            }

            // Handle common prefixes if present in the token
            let clean_token = token
                .trim_start_matches("Bearer ")
                .trim_start_matches("Bearer")
                .trim_start_matches("token ")
                .trim_start_matches("token");

            let mut hasher = Sha256::new();
            hasher.update(clean_token.as_bytes());
            let hash = crate::crypto::hex_encode(hasher.finalize());

            if let Some(agent_id) = hashes.get(&hash) {
                return Some((agent_id.clone(), hash));
            }
        }
        None
    }
}

impl Default for LeakDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SecretLease {
    pub token: String,
    pub secret_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_value: Option<String>,
    pub agent_id: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl fmt::Debug for SecretLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretLease")
            .field("token", &"[REDACTED]")
            .field("secret_name", &"[REDACTED]")
            .field("secret_value", &"[REDACTED]")
            .field("agent_id", &self.agent_id)
            .field("expires_at", &self.expires_at)
            .field("created_at", &self.created_at)
            .finish()
    }
}

impl SecretLease {
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

use crate::secrets::lending::AuditLogger;

pub struct KeyLendingEngine {
    leases: Arc<RwLock<HashMap<String, SecretLease>>>,
    audit_logger: Box<dyn AuditLogger + Send + Sync>,
    pub leak_detector: Arc<LeakDetector>,
    event_bus: Option<crate::coordination::events::XavierEventBus>,
}

impl KeyLendingEngine {
    pub fn new(
        audit_logger: Box<dyn AuditLogger + Send + Sync>,
        event_bus: Option<crate::coordination::events::XavierEventBus>,
    ) -> Self {
        Self {
            leases: Arc::new(RwLock::new(HashMap::new())),
            audit_logger,
            leak_detector: Arc::new(LeakDetector::new()),
            event_bus,
        }
    }

    /// Lend a secret to an agent for a specific duration (TTL)
    pub async fn lend(
        &self,
        name: &str,
        value: Option<&str>,
        agent_id: &str,
        ttl_secs: u64,
    ) -> Result<SecretLease> {
        let token = Uuid::new_v4().to_string();
        let now = Utc::now();
        let expires_at = now + Duration::seconds(ttl_secs as i64);

        let lease = SecretLease {
            token: token.clone(),
            secret_name: name.to_string(),
            secret_value: value.map(|v| v.to_string()),
            agent_id: agent_id.to_string(),
            expires_at,
            created_at: now,
        };

        let mut leases = self.leases.write().await;
        leases.insert(token.clone(), lease.clone());

        if let Some(val) = value {
            self.leak_detector.register_key(val, agent_id).await;
        }

        self.audit_logger.log_lend(agent_id, name, &token, ttl_secs);
        tracing::info!(
            "Lent secret '{}' to agent '{}'. Lease token: {}",
            name,
            agent_id,
            lease.token
        );
        Ok(lease)
    }

    /// Lend a secret from the hardware vault by name
    pub async fn lend_from_vault(
        &self,
        name: &str,
        agent_id: &str,
        ttl_secs: u64,
        redact: bool,
    ) -> Result<SecretLease> {
        let vault = crate::secrets::vault::HardwareVault::new("xavier");
        let value = vault.get_secret(name)?;

        let mut lease = self.lend(name, Some(&value), agent_id, ttl_secs).await?;

        if redact {
            lease.secret_value = None;
        }

        Ok(lease)
    }

    /// Revoke a lease immediately
    pub async fn revoke(&self, token: &str, reason: &str) -> Result<()> {
        let mut leases = self.leases.write().await;
        if let Some(lease) = leases.remove(token) {
            self.audit_logger.log_revoke(&lease.agent_id, token, reason);
            tracing::info!("Revoked secret lease: {} (Reason: {})", token, reason);

            if let Some(bus) = &self.event_bus {
                let _ = bus.publish(crate::coordination::events::XavierEvent::LeaseRevoked {
                    agent_id: lease.agent_id.clone(),
                    token: token.to_string(),
                });
            }

            Ok(())
        } else {
            Err(anyhow!("Lease token not found"))
        }
    }

    /// Renew all leases for a specific agent
    pub async fn renew_for_agent(&self, agent_id: &str, ttl_secs: u64) -> usize {
        let mut leases = self.leases.write().await;
        let mut count = 0;
        for lease in leases.values_mut() {
            if lease.agent_id == agent_id {
                let now = Utc::now();
                lease.expires_at = now + Duration::seconds(ttl_secs as i64);
                count += 1;
            }
        }
        if count > 0 {
            tracing::info!(
                "Renewed {} leases for agent '{}' (New TTL: {}s)",
                count,
                agent_id,
                ttl_secs
            );
        }
        count
    }

    /// Revoke all leases for a specific agent
    pub async fn revoke_for_agent(&self, agent_id: &str, reason: &str) -> usize {
        let mut leases = self.leases.write().await;
        let mut tokens_to_remove = Vec::new();
        for (token, lease) in leases.iter() {
            if lease.agent_id == agent_id {
                tokens_to_remove.push(token.clone());
            }
        }

        let count = tokens_to_remove.len();
        for token in tokens_to_remove {
            leases.remove(&token);
            self.audit_logger.log_revoke(agent_id, &token, reason);

            if let Some(bus) = &self.event_bus {
                let _ = bus.publish(crate::coordination::events::XavierEvent::LeaseRevoked {
                    agent_id: agent_id.to_string(),
                    token: token.clone(),
                });
            }
        }

        if count > 0 {
            tracing::info!(
                "Revoked {} leases for agent '{}' (Reason: {})",
                count,
                agent_id,
                reason
            );
        }
        count
    }

    /// Get lease details by token
    pub async fn get_lease(&self, token: &str) -> Option<SecretLease> {
        let leases = self.leases.read().await;
        leases.get(token).cloned()
    }

    /// Renew a lease for a specific TTL
    pub async fn renew(&self, token: &str, ttl_secs: u64) -> Result<()> {
        let mut leases = self.leases.write().await;
        if let Some(lease) = leases.get_mut(token) {
            let now = Utc::now();
            lease.expires_at = now + Duration::seconds(ttl_secs as i64);
            tracing::info!("Renewed secret lease: {} (New TTL: {}s)", token, ttl_secs);
            Ok(())
        } else {
            Err(anyhow!("Lease token not found"))
        }
    }

    /// Add backoff time to a lease
    pub async fn backoff(&self, token: &str, seconds: u64) -> Result<()> {
        let mut leases = self.leases.write().await;
        if let Some(lease) = leases.get_mut(token) {
            let now = Utc::now();
            let base = if lease.is_expired() {
                now
            } else {
                lease.expires_at
            };
            lease.expires_at = base + Duration::seconds(seconds as i64);
            tracing::info!("Applied backoff to secret lease: {} (+{}s)", token, seconds);
            Ok(())
        } else {
            Err(anyhow!("Lease token not found"))
        }
    }

    /// List all active leases
    pub async fn list_leases(&self) -> Vec<SecretLease> {
        let leases = self.leases.read().await;
        leases.values().cloned().collect()
    }

    /// Log proxy use for a lease token
    pub fn log_proxy_use(&self, agent_id: &str, lease_token: &str, endpoint: &str) {
        self.audit_logger
            .log_proxy_use(agent_id, lease_token, endpoint);
    }

    /// Cleanup expired leases
    pub async fn cleanup_expired(&self) -> usize {
        let mut leases = self.leases.write().await;
        let mut tokens_to_remove = Vec::new();
        for (token, lease) in leases.iter() {
            if lease.is_expired() {
                tokens_to_remove.push(token.clone());
            }
        }

        let count = tokens_to_remove.len();
        for token in tokens_to_remove {
            if let Some(lease) = leases.remove(&token) {
                self.audit_logger
                    .log_revoke(&lease.agent_id, &token, "TTL Expired");
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::lending::AuditLogger;

    struct MockAuditLogger;
    impl AuditLogger for MockAuditLogger {
        fn log_lend(
            &self,
            _agent_id: &str,
            _secret_name: &str,
            _lease_token: &str,
            _ttl_secs: u64,
        ) {
        }
        fn log_revoke(&self, _agent_id: &str, _lease_token: &str, _reason: &str) {}
        fn log_proxy_use(&self, _agent_id: &str, _lease_token: &str, _endpoint: &str) {}
    }

    #[tokio::test]
    async fn test_leak_detector() {
        let detector = LeakDetector::new();
        let secret = "sk-ant-api03-abcdef1234567890";
        let agent_id = "agent-42";

        detector.register_key(secret, agent_id).await;

        // Test exact match
        let content = format!("Sending request with key: {}", secret);
        let leak = detector.check_leak(&content).await;
        assert!(leak.is_some());
        assert_eq!(leak.unwrap().0, agent_id);

        // Test with Bearer prefix
        let content_bearer = format!("Authorization: Bearer {}", secret);
        let leak_bearer = detector.check_leak(&content_bearer).await;
        assert!(leak_bearer.is_some());

        // Test no leak
        let safe_content = "This is a safe message without any keys.";
        let no_leak = detector.check_leak(safe_content).await;
        assert!(no_leak.is_none());
    }

    #[tokio::test]
    async fn test_key_lending_engine_leak_registration() {
        let engine = KeyLendingEngine::new(Box::new(MockAuditLogger), None);
        let secret = "secret-value";
        let agent_id = "agent-1";

        engine
            .lend("test-secret", Some(secret), agent_id, 3600)
            .await
            .unwrap();

        let leak = engine.leak_detector.check_leak(secret).await;
        assert!(leak.is_some());
        assert_eq!(leak.unwrap().0, agent_id);
    }

    #[test]
    fn test_secret_lease_serialization_redaction() {
        let now = Utc::now();
        let lease = SecretLease {
            token: "test-token".to_string(),
            secret_name: "test-secret".to_string(),
            secret_value: None,
            agent_id: "test-agent".to_string(),
            expires_at: now,
            created_at: now,
        };

        let json = serde_json::to_string(&lease).unwrap();
        assert!(!json.contains("secret_value"));
    }

    #[tokio::test]
    async fn test_lend_returns_value_by_default() {
        let engine = KeyLendingEngine::new(Box::new(MockAuditLogger), None);
        let secret = "secret-value";
        let lease = engine.lend("test-secret", Some(secret), "agent-1", 3600).await.unwrap();
        assert_eq!(lease.secret_value, Some(secret.to_string()));
    }

    #[tokio::test]
    async fn test_auto_revocation_on_task_complete() {
        use crate::coordination::events::{XavierEvent, XavierEventBus};
        use crate::coordination::agent_registry::SimpleAgentRegistry;
        use crate::ports::inbound::AgentLifecyclePort;

        let event_bus = XavierEventBus::new(10);
        let engine = Arc::new(KeyLendingEngine::new(Box::new(MockAuditLogger), Some(event_bus.clone())));
        let registry = SimpleAgentRegistry::new_with_engines(
            Some(engine.clone()),
            Some(event_bus.clone()),
        );
        let agent_id = "test-agent-lifecycle";

        // Lend a secret
        let lease = engine.lend("test-secret", Some("val"), agent_id, 3600).await.unwrap();
        assert!(engine.get_lease(&lease.token).await.is_some());

        // Setup the listener (mimicking server.rs)
        let engine_clone = engine.clone();
        let mut receiver = event_bus.subscribe();
        let handle = tokio::spawn(async move {
            if let Ok(event) = receiver.recv().await {
                if let XavierEvent::AgentTaskCompleted { agent_id: id } = event {
                    if id == agent_id {
                        engine_clone.revoke_for_agent(&id, "Task Completed").await;
                    }
                }
            }
        });

        // Trigger task completion with 3-arg API
        let ok_result: Result<crate::agents::runtime::AgentResponse, String> = Ok(crate::agents::runtime::AgentResponse {
            content: "ok".to_string(),
            tool_calls: vec![],
            metadata: std::collections::HashMap::new(),
        });
        registry.on_task_complete(agent_id, "task-1", &ok_result).await;

        // Wait for listener to process
        let _ = tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        drop(handle);

        // Verify revocation
        assert!(engine.get_lease(&lease.token).await.is_none());
    }
}
