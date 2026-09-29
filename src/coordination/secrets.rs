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
use tokio::sync::{OnceCell, RwLock};
use uuid::Uuid;

pub struct LeakDetector {
    /// Map of SHA-256 hashes of secrets to the agent_id they were lent to
    hashes: Arc<RwLock<HashMap<String, String>>>,
}

impl LeakDetector {
    /// New.
    pub fn new() -> Self {
        Self {
            hashes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register key.
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
    /// Is expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

use crate::secrets::lending::{AntiExfilDetector, AuditLogger};

pub struct KeyLendingEngine {
    leases: Arc<RwLock<HashMap<String, SecretLease>>>,
    audit_logger: Box<dyn AuditLogger + Send + Sync>,
    pub leak_detector: Arc<LeakDetector>,
    event_bus: Option<crate::coordination::events::XavierEventBus>,
    anti_exfil: Option<RwLock<AntiExfilDetector>>,
    /// Metrics-database project id `secret_leases` rows are persisted under.
    leases_project_id: String,
    /// Guards the one-time restore of persisted leases into `leases`.
    restored: OnceCell<()>,
}

impl KeyLendingEngine {
    /// New.
    pub fn new(
        audit_logger: Box<dyn AuditLogger + Send + Sync>,
        event_bus: Option<crate::coordination::events::XavierEventBus>,
    ) -> Self {
        Self {
            leases: Arc::new(RwLock::new(HashMap::new())),
            audit_logger,
            leak_detector: Arc::new(LeakDetector::new()),
            event_bus,
            anti_exfil: None,
            leases_project_id: "metrics".to_string(),
            restored: OnceCell::new(),
        }
    }

    /// Attaches an anti-exfiltration rate limiter to this engine's `lend` path.
    /// Lends that exceed the detector's per-agent budget are rejected with
    /// `SecretError::ApprovalDenied` before any lease is created.
    pub fn with_anti_exfil(mut self, detector: AntiExfilDetector) -> Self {
        self.anti_exfil = Some(RwLock::new(detector));
        self
    }

    /// Repopulates `leases` from persisted, non-revoked rows on first use.
    /// Unexpired rows are restored as-is; rows that lapsed while the process
    /// was down are audited as revoked (`reason: "restored-expired"`).
    async fn restore_leases(&self) {
        let rows = match crate::secrets::audit::active_leases(&self.leases_project_id).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("Failed to restore secret leases from store: {}", e);
                return;
            }
        };

        let now = Utc::now();
        let mut lapsed = Vec::new();
        {
            let mut leases = self.leases.write().await;
            for row in rows {
                if row.expires_at > now {
                    leases.insert(
                        row.token.clone(),
                        SecretLease {
                            token: row.token,
                            secret_name: row.secret_name,
                            secret_value: None,
                            agent_id: row.agent_id,
                            expires_at: row.expires_at,
                            created_at: row.created_at,
                        },
                    );
                } else {
                    lapsed.push(row);
                }
            }
        }

        for row in lapsed {
            self.audit_logger
                .log_revoke(&row.agent_id, &row.token, "restored-expired");
            if let Err(e) =
                crate::secrets::audit::mark_lease_revoked(&self.leases_project_id, &row.token).await
            {
                tracing::warn!(
                    "Failed to mark restored-expired lease {} revoked: {}",
                    row.token,
                    e
                );
            }
        }
    }

    /// Runs `restore_leases` at most once per engine instance.
    async fn ensure_restored(&self) {
        self.restored.get_or_init(|| self.restore_leases()).await;
    }

    /// Lend a secret to an agent for a specific duration (TTL)
    pub async fn lend(
        &self,
        name: &str,
        value: Option<&str>,
        agent_id: &str,
        ttl_secs: u64,
    ) -> Result<SecretLease> {
        self.check_anti_exfil(agent_id, name).await?;
        self.lend_unchecked(name, value, agent_id, ttl_secs).await
    }

    /// Runs the anti-exfiltration rate-limit check for `agent_id` and records
    /// the attempt. Every entry point that may resolve a secret value (from a
    /// caller-supplied value or from the hardware vault) must call this
    /// *before* resolving anything, so a rate-limited agent never causes a
    /// real vault read.
    async fn check_anti_exfil(&self, agent_id: &str, name: &str) -> Result<()> {
        if let Some(detector) = &self.anti_exfil {
            let mut detector = detector.write().await;
            if let Err(err) = detector.check_and_record(agent_id) {
                self.audit_logger.log_deny(agent_id, name, &err.to_string());
                return Err(err.into());
            }
        }
        Ok(())
    }

    /// Creates the lease and records it. Callers must have already run
    /// `check_anti_exfil` for `agent_id`; this does not check again so a
    /// single lend attempt is only ever counted once against the budget.
    async fn lend_unchecked(
        &self,
        name: &str,
        value: Option<&str>,
        agent_id: &str,
        ttl_secs: u64,
    ) -> Result<SecretLease> {
        self.ensure_restored().await;

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

        {
            let mut leases = self.leases.write().await;
            leases.insert(token.clone(), lease.clone());
        }

        if let Some(val) = value {
            self.leak_detector.register_key(val, agent_id).await;
        }

        self.audit_logger.log_lend(agent_id, name, &token, ttl_secs);
        if let Err(e) = crate::secrets::audit::insert_lease(
            &self.leases_project_id,
            &token,
            name,
            agent_id,
            now,
            expires_at,
        )
        .await
        {
            tracing::warn!("Failed to persist secret lease {}: {}", token, e);
        }
        tracing::info!(
            "Lent secret '{}' to agent '{}'. Lease token: {}",
            name,
            agent_id,
            lease.token
        );
        Ok(lease)
    }

    /// Lend a secret from the hardware vault by name.
    ///
    /// The anti-exfil check runs before the vault is touched: a rate-limited
    /// agent is rejected without ever causing a `HardwareVault::get_secret`
    /// call, so repeated denied attempts cannot pull secret material into
    /// process memory.
    pub async fn lend_from_vault(
        &self,
        name: &str,
        agent_id: &str,
        ttl_secs: u64,
        redact: bool,
    ) -> Result<SecretLease> {
        self.check_anti_exfil(agent_id, name).await?;

        let vault = crate::secrets::vault::HardwareVault::new("xavier");
        let value = vault.get_secret(name)?;

        let mut lease = self
            .lend_unchecked(name, Some(&value), agent_id, ttl_secs)
            .await?;

        if redact {
            lease.secret_value = None;
        }

        Ok(lease)
    }

    /// Revoke a lease immediately
    pub async fn revoke(&self, token: &str, reason: &str) -> Result<()> {
        self.ensure_restored().await;

        let removed = {
            let mut leases = self.leases.write().await;
            leases.remove(token)
        };

        if let Some(lease) = removed {
            self.audit_logger.log_revoke(&lease.agent_id, token, reason);
            if let Err(e) =
                crate::secrets::audit::mark_lease_revoked(&self.leases_project_id, token).await
            {
                tracing::warn!("Failed to persist lease revoke for {}: {}", token, e);
            }
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
        self.ensure_restored().await;

        let renewed: Vec<(String, DateTime<Utc>)> = {
            let mut leases = self.leases.write().await;
            let new_expiry = Utc::now() + Duration::seconds(ttl_secs as i64);
            leases
                .values_mut()
                .filter(|lease| lease.agent_id == agent_id)
                .map(|lease| {
                    lease.expires_at = new_expiry;
                    (lease.token.clone(), new_expiry)
                })
                .collect()
        };

        for (token, expires_at) in &renewed {
            if let Err(e) = crate::secrets::audit::update_lease_expiry(
                &self.leases_project_id,
                token,
                *expires_at,
            )
            .await
            {
                tracing::warn!("Failed to persist lease renewal for {}: {}", token, e);
            }
        }

        let count = renewed.len();
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
        self.ensure_restored().await;

        let removed: Vec<SecretLease> = {
            let mut leases = self.leases.write().await;
            let tokens_to_remove: Vec<String> = leases
                .iter()
                .filter(|(_, lease)| lease.agent_id == agent_id)
                .map(|(token, _)| token.clone())
                .collect();
            tokens_to_remove
                .into_iter()
                .filter_map(|token| leases.remove(&token))
                .collect()
        };

        for lease in &removed {
            self.audit_logger.log_revoke(agent_id, &lease.token, reason);
            if let Err(e) =
                crate::secrets::audit::mark_lease_revoked(&self.leases_project_id, &lease.token)
                    .await
            {
                tracing::warn!("Failed to persist lease revoke for {}: {}", lease.token, e);
            }

            if let Some(bus) = &self.event_bus {
                let _ = bus.publish(crate::coordination::events::XavierEvent::LeaseRevoked {
                    agent_id: agent_id.to_string(),
                    token: lease.token.clone(),
                });
            }
        }

        let count = removed.len();
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
        self.ensure_restored().await;
        let leases = self.leases.read().await;
        leases.get(token).cloned()
    }

    /// Renew a lease for a specific TTL
    pub async fn renew(&self, token: &str, ttl_secs: u64) -> Result<()> {
        self.ensure_restored().await;

        let new_expiry = {
            let mut leases = self.leases.write().await;
            leases.get_mut(token).map(|lease| {
                let now = Utc::now();
                lease.expires_at = now + Duration::seconds(ttl_secs as i64);
                lease.expires_at
            })
        };

        if let Some(expires_at) = new_expiry {
            if let Err(e) = crate::secrets::audit::update_lease_expiry(
                &self.leases_project_id,
                token,
                expires_at,
            )
            .await
            {
                tracing::warn!("Failed to persist lease renewal for {}: {}", token, e);
            }
            tracing::info!("Renewed secret lease: {} (New TTL: {}s)", token, ttl_secs);
            Ok(())
        } else {
            Err(anyhow!("Lease token not found"))
        }
    }

    /// Add backoff time to a lease
    pub async fn backoff(&self, token: &str, seconds: u64) -> Result<()> {
        self.ensure_restored().await;

        let new_expiry = {
            let mut leases = self.leases.write().await;
            leases.get_mut(token).map(|lease| {
                let now = Utc::now();
                let base = if lease.is_expired() {
                    now
                } else {
                    lease.expires_at
                };
                lease.expires_at = base + Duration::seconds(seconds as i64);
                lease.expires_at
            })
        };

        if let Some(expires_at) = new_expiry {
            if let Err(e) = crate::secrets::audit::update_lease_expiry(
                &self.leases_project_id,
                token,
                expires_at,
            )
            .await
            {
                tracing::warn!("Failed to persist lease backoff for {}: {}", token, e);
            }
            tracing::info!("Applied backoff to secret lease: {} (+{}s)", token, seconds);
            Ok(())
        } else {
            Err(anyhow!("Lease token not found"))
        }
    }

    /// List all active leases
    pub async fn list_leases(&self) -> Vec<SecretLease> {
        self.ensure_restored().await;
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
        self.ensure_restored().await;

        let removed: Vec<SecretLease> = {
            let mut leases = self.leases.write().await;
            let tokens_to_remove: Vec<String> = leases
                .iter()
                .filter(|(_, lease)| lease.is_expired())
                .map(|(token, _)| token.clone())
                .collect();
            tokens_to_remove
                .into_iter()
                .filter_map(|token| leases.remove(&token))
                .collect()
        };

        for lease in &removed {
            self.audit_logger
                .log_revoke(&lease.agent_id, &lease.token, "TTL Expired");
            if let Err(e) =
                crate::secrets::audit::mark_lease_revoked(&self.leases_project_id, &lease.token)
                    .await
            {
                tracing::warn!("Failed to persist lease cleanup for {}: {}", lease.token, e);
            }
        }
        removed.len()
    }
}

#[cfg(test)]
impl KeyLendingEngine {
    /// Points lease persistence at an already-connected project id instead of
    /// the default `"metrics"` database. Test-only: lets restart tests reopen
    /// a second engine over the same tempdir-scoped database without ever
    /// touching the real `metrics.db`.
    fn with_leases_project(mut self, project_id: impl Into<String>) -> Self {
        self.leases_project_id = project_id.into();
        self
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
        let (project_id, _dir) = isolated_leases_project("leak-registration");
        let engine =
            KeyLendingEngine::new(Box::new(MockAuditLogger), None).with_leases_project(project_id);
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
        let (project_id, _dir) = isolated_leases_project("lend-default-value");
        let engine =
            KeyLendingEngine::new(Box::new(MockAuditLogger), None).with_leases_project(project_id);
        let secret = "secret-value";
        let lease = engine
            .lend("test-secret", Some(secret), "agent-1", 3600)
            .await
            .unwrap();
        assert_eq!(lease.secret_value, Some(secret.to_string()));
    }

    #[tokio::test]
    async fn test_auto_revocation_on_task_complete() {
        use crate::coordination::agent_registry::SimpleAgentRegistry;
        use crate::coordination::events::{XavierEvent, XavierEventBus};
        use crate::ports::inbound::AgentLifecyclePort;

        let event_bus = XavierEventBus::new(10);
        let (project_id, _dir) = isolated_leases_project("auto-revocation");
        let engine = Arc::new(
            KeyLendingEngine::new(Box::new(MockAuditLogger), Some(event_bus.clone()))
                .with_leases_project(project_id),
        );
        let registry =
            SimpleAgentRegistry::new_with_engines(Some(engine.clone()), Some(event_bus.clone()));
        let agent_id = "test-agent-lifecycle";

        // Lend a secret
        let lease = engine
            .lend("test-secret", Some("val"), agent_id, 3600)
            .await
            .unwrap();
        assert!(engine.get_lease(&lease.token).await.is_some());

        // Setup the listener (mimicking server.rs)
        let engine_clone = engine.clone();
        let mut receiver = event_bus.subscribe();
        let handle = tokio::spawn(async move {
            if let Ok(XavierEvent::AgentTaskCompleted { agent_id: id, .. }) = receiver.recv().await
            {
                if id == agent_id {
                    engine_clone.revoke_for_agent(&id, "Task Completed").await;
                }
            }
        });

        // Trigger task completion with 3-arg API
        let ok_result: Result<crate::agents::runtime::AgentResponse, String> =
            Ok(crate::agents::runtime::AgentResponse {
                session_id: "task-1".to_string(),
                query: "test".to_string(),
                response: "ok".to_string(),
                confidence: 1.0,
                system_timings: crate::agents::runtime::SystemTimings {
                    system1_ms: 0,
                    system2_ms: 0,
                    system3_ms: 0,
                    total_ms: 0,
                },
            });
        registry
            .on_task_complete(agent_id, "task-1", &ok_result)
            .await;

        // Wait for listener to process
        let _ = tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        drop(handle);

        // Verify revocation
        assert!(engine.get_lease(&lease.token).await.is_none());
    }

    #[tokio::test]
    async fn test_lend_rate_limited_by_anti_exfil() {
        let (project_id, _dir) = isolated_leases_project("rate-limited");
        let engine = KeyLendingEngine::new(Box::new(MockAuditLogger), None)
            .with_leases_project(project_id)
            .with_anti_exfil(crate::secrets::lending::AntiExfilDetector::new(10));

        for _ in 0..10 {
            engine
                .lend("test-secret", Some("val"), "agent-1", 3600)
                .await
                .unwrap();
        }

        let err = engine
            .lend("test-secret", Some("val"), "agent-1", 3600)
            .await
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<crate::secrets::SecretError>(),
            Some(crate::secrets::SecretError::ApprovalDenied(_))
        ));
    }

    #[tokio::test]
    async fn test_rate_limit_is_per_agent() {
        let (project_id, _dir) = isolated_leases_project("rate-limit-per-agent");
        let engine = KeyLendingEngine::new(Box::new(MockAuditLogger), None)
            .with_leases_project(project_id)
            .with_anti_exfil(crate::secrets::lending::AntiExfilDetector::new(10));

        for _ in 0..10 {
            engine
                .lend("test-secret", Some("val"), "agent-1", 3600)
                .await
                .unwrap();
        }

        // agent-2 has its own budget and is unaffected by agent-1's usage.
        let lease = engine
            .lend("test-secret", Some("val"), "agent-2", 3600)
            .await
            .unwrap();
        assert_eq!(lease.agent_id, "agent-2");
    }

    #[tokio::test]
    async fn test_rejected_lend_creates_no_lease() {
        let (project_id, _dir) = isolated_leases_project("rejected-lend");
        let engine = KeyLendingEngine::new(Box::new(MockAuditLogger), None)
            .with_leases_project(project_id)
            .with_anti_exfil(crate::secrets::lending::AntiExfilDetector::new(0));

        let err = engine
            .lend("test-secret", Some("val"), "agent-1", 3600)
            .await
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<crate::secrets::SecretError>(),
            Some(crate::secrets::SecretError::ApprovalDenied(_))
        ));
        assert!(engine.list_leases().await.is_empty());
    }

    #[tokio::test]
    async fn test_lend_from_vault_rejected_before_vault_read() {
        // Budget of 0 means the very first attempt is rejected. If the
        // rejection happened after the vault read, this call would instead
        // fail with a vault/secret-not-found error from HardwareVault --
        // touching the real keyring/fallback store in the process.
        let (project_id, _dir) = isolated_leases_project("lend-from-vault-rejected");
        let engine = KeyLendingEngine::new(Box::new(MockAuditLogger), None)
            .with_leases_project(project_id)
            .with_anti_exfil(crate::secrets::lending::AntiExfilDetector::new(0));

        let err = engine
            .lend_from_vault("does-not-exist", "agent-1", 3600, true)
            .await
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<crate::secrets::SecretError>(),
            Some(crate::secrets::SecretError::ApprovalDenied(_))
        ));
        assert!(engine.list_leases().await.is_empty());
    }

    /// Records every `log_revoke` call so restore tests can assert the
    /// `reason` passed for a lease that lapsed while the process was down,
    /// without polling a real audit table.
    #[derive(Default)]
    struct RecordingAuditLogger {
        revokes: std::sync::Mutex<Vec<(String, String, String)>>,
    }

    impl AuditLogger for Arc<RecordingAuditLogger> {
        fn log_lend(
            &self,
            _agent_id: &str,
            _secret_name: &str,
            _lease_token: &str,
            _ttl_secs: u64,
        ) {
        }
        fn log_revoke(&self, agent_id: &str, lease_token: &str, reason: &str) {
            self.revokes.lock().unwrap().push((
                agent_id.to_string(),
                lease_token.to_string(),
                reason.to_string(),
            ));
        }
        fn log_proxy_use(&self, _agent_id: &str, _lease_token: &str, _endpoint: &str) {}
    }

    /// Connects a fresh, tempdir-scoped project id so lease-persistence
    /// tests never share the real `metrics.db` used by production and other
    /// test modules. The returned `TempDir` must outlive the engine(s) using
    /// the connection.
    fn isolated_leases_project(name: &str) -> (String, tempfile::TempDir) {
        let project_id = format!("test_secrets_{}_{}", name, Uuid::new_v4());
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("metrics.db");
        crate::codebase::connection_manager::ConnectionManager::global()
            .connect_with_path(&project_id, db_path)
            .expect("connect isolated metrics db");
        (project_id, dir)
    }

    #[tokio::test]
    async fn test_lease_persisted_on_lend() {
        let (project_id, _dir) = isolated_leases_project("persist");
        let engine = KeyLendingEngine::new(Box::new(MockAuditLogger), None)
            .with_leases_project(project_id.clone());

        let lease = engine
            .lend("db-password", Some("s3cr3t"), "agent-persist", 3600)
            .await
            .unwrap();

        let rows = crate::secrets::audit::active_leases(&project_id)
            .await
            .expect("active leases");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].token, lease.token);
        assert_eq!(rows[0].secret_name, "db-password");
        assert_eq!(rows[0].agent_id, "agent-persist");
    }

    #[tokio::test]
    async fn test_lease_marked_revoked_in_store() {
        let (project_id, _dir) = isolated_leases_project("revoke");
        let engine = KeyLendingEngine::new(Box::new(MockAuditLogger), None)
            .with_leases_project(project_id.clone());

        let lease = engine
            .lend("api-key", Some("s3cr3t"), "agent-revoke", 3600)
            .await
            .unwrap();
        engine.revoke(&lease.token, "manual revoke").await.unwrap();

        let rows = crate::secrets::audit::active_leases(&project_id)
            .await
            .expect("active leases");
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn test_restore_repopulates_unexpired_leases() {
        let (project_id, _dir) = isolated_leases_project("restore-active");
        let engine_a = KeyLendingEngine::new(Box::new(MockAuditLogger), None)
            .with_leases_project(project_id.clone());
        let lease = engine_a
            .lend("still-valid", Some("s3cr3t"), "agent-restore", 3600)
            .await
            .unwrap();
        drop(engine_a);

        // A fresh engine over the same tempdir-scoped database -- the
        // restart scenario: nothing in this engine's memory yet.
        let engine_b = KeyLendingEngine::new(Box::new(MockAuditLogger), None)
            .with_leases_project(project_id.clone());
        let restored = engine_b.list_leases().await;
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].token, lease.token);
        assert_eq!(restored[0].agent_id, "agent-restore");
        assert_eq!(restored[0].secret_value, None);
    }

    #[tokio::test]
    async fn test_restore_drops_expired_leases_and_audits_them() {
        let (project_id, _dir) = isolated_leases_project("restore-expired");
        // Seed a lease whose TTL lapsed while "the process was down" --
        // written directly to the store, bypassing any engine's clock.
        let past = Utc::now() - Duration::seconds(120);
        crate::secrets::audit::insert_lease(
            &project_id,
            "lapsed-token",
            "lapsed-secret",
            "agent-lapsed",
            past - Duration::seconds(60),
            past,
        )
        .await
        .expect("seed lapsed lease");

        let logger = Arc::new(RecordingAuditLogger::default());
        let engine = KeyLendingEngine::new(Box::new(logger.clone()), None)
            .with_leases_project(project_id.clone());

        let restored = engine.list_leases().await;
        assert!(restored.is_empty());

        let revoked = {
            let revokes = logger.revokes.lock().unwrap();
            revokes.iter().any(|(agent, token, reason)| {
                agent == "agent-lapsed" && token == "lapsed-token" && reason == "restored-expired"
            })
        };
        assert!(revoked);

        let rows = crate::secrets::audit::active_leases(&project_id)
            .await
            .expect("active leases");
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn test_restart_scenario_lists_unexpired_and_drops_expired() {
        let (project_id, _dir) = isolated_leases_project("restart");
        let engine_a = KeyLendingEngine::new(Box::new(MockAuditLogger), None)
            .with_leases_project(project_id.clone());
        let kept = engine_a
            .lend("kept-secret", Some("s3cr3t"), "agent-kept", 3600)
            .await
            .unwrap();
        let past = Utc::now() - Duration::seconds(60);
        crate::secrets::audit::insert_lease(
            &project_id,
            "dropped-token",
            "dropped-secret",
            "agent-dropped",
            past - Duration::seconds(60),
            past,
        )
        .await
        .expect("seed lapsed lease");
        drop(engine_a);

        let engine_b = KeyLendingEngine::new(Box::new(MockAuditLogger), None)
            .with_leases_project(project_id.clone());
        let restored = engine_b.list_leases().await;
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].token, kept.token);

        let rows = crate::secrets::audit::active_leases(&project_id)
            .await
            .expect("active leases");
        assert!(rows.iter().all(|r| r.token != "dropped-token"));
    }
}
