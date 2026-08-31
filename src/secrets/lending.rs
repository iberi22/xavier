//! Secret lending and temporary access
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::SecretError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

#[derive(Clone)]
pub struct EphemeralLease {
    pub session_token: String,
    pub real_secret_id: String,
    pub agent_id: String,
    pub expires_at: SystemTime,
}

impl fmt::Debug for EphemeralLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EphemeralLease")
            .field("session_token", &"[REDACTED]")
            .field("real_secret_id", &"[REDACTED]")
            .field("agent_id", &self.agent_id)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

pub trait AuditLogger {
    fn log_lend(&self, agent_id: &str, secret_id: &str, session_token: &str, ttl_secs: u64);
    fn log_revoke(&self, agent_id: &str, session_token: &str, reason: &str);
    fn log_proxy_use(&self, agent_id: &str, lease_token: &str, endpoint: &str);
}

pub struct DefaultAuditLogger;

impl AuditLogger for DefaultAuditLogger {
    fn log_lend(&self, agent_id: &str, secret_id: &str, session_token: &str, ttl_secs: u64) {
        println!(
            "[AUDIT] LENT secret '{}' to agent '{}' (Session: {}, TTL: {}s)",
            secret_id, agent_id, session_token, ttl_secs
        );
    }
    fn log_revoke(&self, agent_id: &str, session_token: &str, reason: &str) {
        println!(
            "[AUDIT] REVOKED session '{}' for agent '{}' (Reason: {})",
            session_token, agent_id, reason
        );
    }
    fn log_proxy_use(&self, agent_id: &str, lease_token: &str, endpoint: &str) {
        println!(
            "[AUDIT] PROXY USE by agent '{}' with lease '{}' on endpoint '{}'",
            agent_id, lease_token, endpoint
        );
    }
}

pub struct KeyLendingEngine<A: AuditLogger> {
    leases: HashMap<String, EphemeralLease>,
    audit_logger: A,
}

impl<A: AuditLogger> KeyLendingEngine<A> {
    /// New.
    pub fn new(audit_logger: A) -> Self {
        Self {
            leases: HashMap::new(),
            audit_logger,
        }
    }

    /// Lend.
    pub fn lend(
        &mut self,
        agent_id: &str,
        real_secret_id: &str,
        ttl_secs: u64,
    ) -> Result<String, SecretError> {
        let session_token = Uuid::new_v4().to_string();
        let expires_at = SystemTime::now() + Duration::from_secs(ttl_secs);

        let lease = EphemeralLease {
            session_token: session_token.clone(),
            real_secret_id: real_secret_id.to_string(),
            agent_id: agent_id.to_string(),
            expires_at,
        };

        self.leases.insert(session_token.clone(), lease);
        self.audit_logger
            .log_lend(agent_id, real_secret_id, &session_token, ttl_secs);

        Ok(session_token)
    }

    /// Revoke.
    pub fn revoke(&mut self, session_token: &str, reason: &str) -> Result<(), SecretError> {
        if let Some(lease) = self.leases.remove(session_token) {
            self.audit_logger
                .log_revoke(&lease.agent_id, session_token, reason);
            Ok(())
        } else {
            Err(SecretError::NotFound(format!(
                "Lease {} not found",
                session_token
            )))
        }
    }

    /// Resolve.
    pub fn resolve(&self, session_token: &str) -> Result<String, SecretError> {
        if let Some(lease) = self.leases.get(session_token) {
            if SystemTime::now() > lease.expires_at {
                return Err(SecretError::ApprovalDenied(
                    "Session token expired".to_string(),
                ));
            }
            Ok(lease.real_secret_id.clone())
        } else {
            Err(SecretError::NotFound(
                "Session token invalid or revoked".to_string(),
            ))
        }
    }

    /// Cleanup expired.
    pub fn cleanup_expired(&mut self) {
        let now = SystemTime::now();
        let mut expired_tokens = Vec::new();

        for (token, lease) in self.leases.iter() {
            if now > lease.expires_at {
                expired_tokens.push(token.clone());
            }
        }

        for token in expired_tokens {
            if let Some(lease) = self.leases.remove(&token) {
                self.audit_logger
                    .log_revoke(&lease.agent_id, &token, "TTL Expired");
            }
        }
    }

    /// Active lease count
    pub fn lease_count(&self) -> usize {
        self.leases.len()
    }
}

/// Vault anti-exfiltration detector + MCP + OpenBao stub (WAVE-3.05)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiExfilDetector {
    pub max_lends_per_minute: u32,
    pub block_external_ips: bool,
    pub recent_lends: Vec<(String, SystemTime)>,
}

impl Default for AntiExfilDetector {
    fn default() -> Self {
        Self {
            max_lends_per_minute: 10,
            block_external_ips: true,
            recent_lends: Vec::new(),
        }
    }
}

impl AntiExfilDetector {
    pub fn new(max_per_min: u32) -> Self {
        Self {
            max_lends_per_minute: max_per_min,
            block_external_ips: true,
            recent_lends: Vec::new(),
        }
    }

    pub fn check_and_record(&mut self, agent_id: &str) -> Result<(), SecretError> {
        let now = SystemTime::now();
        self.recent_lends.retain(|(_, t)| {
            now.duration_since(*t)
                .unwrap_or(Duration::from_secs(999))
                .as_secs()
                < 60
        });
        let count = self
            .recent_lends
            .iter()
            .filter(|(a, _)| a == agent_id)
            .count();
        if count as u32 >= self.max_lends_per_minute {
            return Err(SecretError::ApprovalDenied(format!(
                "Vault anti-exfil: agent {} exceeded {} lends/min",
                agent_id, self.max_lends_per_minute
            )));
        }
        self.recent_lends.push((agent_id.to_string(), now));
        Ok(())
    }

    pub fn is_allowed_ip(&self, ip: &str) -> bool {
        if !self.block_external_ips {
            return true;
        }
        ip.starts_with("127.") || ip.starts_with("10.") || ip.starts_with("192.168.") || ip == "::1"
    }
}

pub fn resolve_via_mcp(secret_id: &str, mcp_endpoint: &str) -> Result<String, SecretError> {
    if mcp_endpoint.is_empty() {
        return Err(SecretError::NotFound("MCP endpoint empty".into()));
    }
    Ok(format!("mcp_resolved:{secret_id}"))
}

pub fn fetch_from_openbao(secret_id: &str, openbao_url: &str) -> Result<String, SecretError> {
    if openbao_url.is_empty() {
        return Err(SecretError::NotFound("OpenBao URL empty".into()));
    }
    Ok(format!("openbao:{secret_id}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultDashboardLease {
    pub session_token_masked: String,
    pub agent_id: String,
    pub expires_in_secs: u64,
    pub is_expired: bool,
}

pub fn dashboard_leases<A: AuditLogger>(engine: &KeyLendingEngine<A>) -> Vec<VaultDashboardLease> {
    engine
        .leases
        .values()
        .map(|l| {
            let is_expired = SystemTime::now() > l.expires_at;
            let expires_in_secs = l
                .expires_at
                .duration_since(SystemTime::now())
                .unwrap_or(Duration::from_secs(0))
                .as_secs();
            VaultDashboardLease {
                session_token_masked: format!(
                    "{}...",
                    &l.session_token[..4.min(l.session_token.len())]
                ),
                agent_id: l.agent_id.clone(),
                expires_in_secs,
                is_expired,
            }
        })
        .collect()
}
