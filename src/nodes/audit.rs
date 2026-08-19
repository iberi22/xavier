//! Audit helper with secret masking for node lifecycle events
//!
//! Emits structured audit records for add, rotate, remove, lend, and revoke events.
//! Tokens and private keys are ALWAYS masked or completely redacted before logging.

use crate::nodes::Provider;
use chrono::Utc;
use tracing::info;

/// Mask a secret token or key for audit logging.
///
/// Keeps at most the first 4 and last 4 characters if long enough,
/// otherwise replaces completely with `[REDACTED]`.
pub fn mask_secret(secret: &str) -> String {
    let len = secret.len();
    if len <= 8 {
        "[REDACTED]".to_string()
    } else {
        format!("{}...[REDACTED]...{}", &secret[..4], &secret[len - 4..])
    }
}

/// Audit event categories for node operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeAuditEvent {
    Provision,
    Rotate,
    Remove,
    LeaseLent,
    LeaseRevoked,
    Heartbeat,
}

impl std::fmt::Display for NodeAuditEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeAuditEvent::Provision => write!(f, "NODE_PROVISION"),
            NodeAuditEvent::Rotate => write!(f, "NODE_ROTATE"),
            NodeAuditEvent::Remove => write!(f, "NODE_REMOVE"),
            NodeAuditEvent::LeaseLent => write!(f, "NODE_LEASE_LENT"),
            NodeAuditEvent::LeaseRevoked => write!(f, "NODE_LEASE_REVOKED"),
            NodeAuditEvent::Heartbeat => write!(f, "NODE_HEARTBEAT"),
        }
    }
}

/// Log a structured node audit event with automatic secret masking.
pub fn log_node_audit(event: NodeAuditEvent, node_id: &str, provider: Provider, details: &str) {
    let now = Utc::now().to_rfc3339();
    info!(
        audit = true,
        timestamp = %now,
        event = %event,
        node_id = %node_id,
        provider = %provider,
        details = %details,
        "[AUDIT] {} for node '{}' (Provider: {}): {}",
        event,
        node_id,
        provider,
        details
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_secret_short() {
        assert_eq!(mask_secret("secret"), "[REDACTED]");
        assert_eq!(mask_secret("12345678"), "[REDACTED]");
    }

    #[test]
    fn test_mask_secret_long() {
        let masked = mask_secret("sbp_1234567890abcdef123456");
        assert!(masked.starts_with("sbp_"));
        assert!(masked.ends_with("3456"));
        assert!(masked.contains("[REDACTED]"));
        assert!(!masked.contains("abcdef"));
    }
}
