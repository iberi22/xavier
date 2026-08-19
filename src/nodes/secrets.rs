//! Secrets management bridge for SWAL nodes
//!
//! Stores node credentials (BaaS API tokens, dedicated VPS SSH private keys)
//! EXCLUSIVELY in `src/secrets/` (HardwareVault / LocalSecretsVault with AES-256-GCM)
//! and administers access via `KeyLendingEngine` and `EphemeralLease`.
//!
//! Credentials are NEVER stored in plaintext on disk, in logs, or in ClavisEngine.

use anyhow::{anyhow, Context, Result};
use std::sync::{Arc, Mutex};

use crate::nodes::audit::{log_node_audit, NodeAuditEvent};
use crate::nodes::Provider;
use crate::secrets::lending::{AuditLogger, DefaultAuditLogger, KeyLendingEngine};
use crate::secrets::vault::HardwareVault;

const NODE_SECRETS_SERVICE: &str = "xavier-node-secrets";

/// Format the persistent secret identifier in the vault for a given node ID.
pub fn secret_key_for_node(node_id: &str) -> String {
    format!("node_secret_{}", node_id)
}

/// Store a node's secret credential in the encrypted vault and issue an ephemeral lease.
pub fn store_node_secret<A: AuditLogger>(
    vault: &HardwareVault,
    lending: &mut KeyLendingEngine<A>,
    node_id: &str,
    provider: Provider,
    secret_value: &str,
    ttl_secs: u64,
) -> Result<String> {
    if secret_value.trim().is_empty() {
        return Err(anyhow!(
            "Cannot store an empty secret for node '{}'",
            node_id
        ));
    }

    let secret_key = secret_key_for_node(node_id);

    // 1. Store in AES-256-GCM encrypted vault
    vault
        .store_secret(&secret_key, secret_value)
        .map_err(|e| anyhow!("Failed to persist secret to vault: {}", e))?;

    // 2. Issue ephemeral lease via KeyLendingEngine
    let agent_id = format!("node:{}", node_id);
    let session_token = lending
        .lend(&agent_id, &secret_key, ttl_secs)
        .map_err(|e| anyhow!("Failed to issue ephemeral lease for secret: {}", e))?;

    log_node_audit(
        NodeAuditEvent::LeaseLent,
        node_id,
        provider,
        &format!("Lease lent (TTL: {}s)", ttl_secs),
    );

    Ok(session_token)
}

/// Resolve and fetch a node's secret credential, validating the ephemeral lease.
pub fn get_node_secret<A: AuditLogger>(
    vault: &HardwareVault,
    lending: &KeyLendingEngine<A>,
    node_id: &str,
    lease_id: &str,
) -> Result<String> {
    // 1. Validate lease token
    let real_secret_id = lending
        .resolve(lease_id)
        .map_err(|e| anyhow!("Lease validation failed: {}", e))?;

    let expected_secret_key = secret_key_for_node(node_id);
    if real_secret_id != expected_secret_key {
        return Err(anyhow!(
            "Lease '{}' does not belong to node '{}'",
            lease_id,
            node_id
        ));
    }

    // 2. Retrieve plaintext from AES-256-GCM vault
    let secret = vault
        .get_secret(&real_secret_id)
        .map_err(|e| anyhow!("Failed to retrieve secret from vault: {}", e))?;

    Ok(secret)
}

/// Rotate a node's secret credential: updates vault and replaces the ephemeral lease.
pub fn rotate_node_secret<A: AuditLogger>(
    vault: &HardwareVault,
    lending: &mut KeyLendingEngine<A>,
    node_id: &str,
    provider: Provider,
    new_secret: &str,
    old_lease_id: Option<&str>,
    ttl_secs: u64,
) -> Result<String> {
    if new_secret.trim().is_empty() {
        return Err(anyhow!(
            "Cannot rotate to an empty secret for node '{}'",
            node_id
        ));
    }

    // 1. Revoke old lease if present
    if let Some(old_lease) = old_lease_id {
        let _ = lending.revoke(old_lease, "Credential rotation");
        log_node_audit(
            NodeAuditEvent::LeaseRevoked,
            node_id,
            provider,
            "Old lease revoked due to rotation",
        );
    }

    // 2. Store new secret and issue new lease
    store_node_secret(vault, lending, node_id, provider, new_secret, ttl_secs)
}

/// Revoke a node's secret credential and lease.
pub fn revoke_node_secret<A: AuditLogger>(
    vault: &HardwareVault,
    lending: &mut KeyLendingEngine<A>,
    node_id: &str,
    provider: Provider,
    lease_id: Option<&str>,
    reason: &str,
    purge_from_vault: bool,
) -> Result<()> {
    if let Some(lease) = lease_id {
        let _ = lending.revoke(lease, reason);
        log_node_audit(
            NodeAuditEvent::LeaseRevoked,
            node_id,
            provider,
            &format!("Lease revoked: {}", reason),
        );
    }

    if purge_from_vault {
        let secret_key = secret_key_for_node(node_id);
        let _ = vault.delete_secret(&secret_key);
    }

    Ok(())
}

/// Self-contained thread-safe manager for node secrets.
#[derive(Clone)]
pub struct NodeSecretsManager {
    vault: Arc<HardwareVault>,
    lending: Arc<Mutex<KeyLendingEngine<DefaultAuditLogger>>>,
}

impl Default for NodeSecretsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeSecretsManager {
    /// Create a new NodeSecretsManager with default HardwareVault and AuditLogger.
    pub fn new() -> Self {
        Self {
            vault: Arc::new(HardwareVault::new(NODE_SECRETS_SERVICE)),
            lending: Arc::new(Mutex::new(KeyLendingEngine::new(DefaultAuditLogger))),
        }
    }

    /// Store a node secret.
    pub fn store(
        &self,
        node_id: &str,
        provider: Provider,
        secret_value: &str,
        ttl_secs: u64,
    ) -> Result<String> {
        let mut lending = self
            .lending
            .lock()
            .map_err(|_| anyhow!("Failed to acquire lending engine lock"))?;
        store_node_secret(
            &self.vault,
            &mut *lending,
            node_id,
            provider,
            secret_value,
            ttl_secs,
        )
    }

    /// Retrieve a node secret.
    pub fn get(&self, node_id: &str, lease_id: &str) -> Result<String> {
        let lending = self
            .lending
            .lock()
            .map_err(|_| anyhow!("Failed to acquire lending engine lock"))?;
        get_node_secret(&self.vault, &*lending, node_id, lease_id)
    }

    /// Rotate a node secret.
    pub fn rotate(
        &self,
        node_id: &str,
        provider: Provider,
        new_secret: &str,
        old_lease_id: Option<&str>,
        ttl_secs: u64,
    ) -> Result<String> {
        let mut lending = self
            .lending
            .lock()
            .map_err(|_| anyhow!("Failed to acquire lending engine lock"))?;
        rotate_node_secret(
            &self.vault,
            &mut *lending,
            node_id,
            provider,
            new_secret,
            old_lease_id,
            ttl_secs,
        )
    }

    /// Revoke a node secret.
    pub fn revoke(
        &self,
        node_id: &str,
        provider: Provider,
        lease_id: Option<&str>,
        reason: &str,
        purge_from_vault: bool,
    ) -> Result<()> {
        let mut lending = self
            .lending
            .lock()
            .map_err(|_| anyhow!("Failed to acquire lending engine lock"))?;
        revoke_node_secret(
            &self.vault,
            &mut *lending,
            node_id,
            provider,
            lease_id,
            reason,
            purge_from_vault,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_secrets_roundtrip_and_revocation() {
        let manager = NodeSecretsManager::new();
        let node_id = "xv1-testnode-secret-roundtrip";
        let provider = Provider::Supabase;
        let token = "sbp_test_token_123456789";

        // Store
        let lease_id = manager.store(node_id, provider, token, 3600).unwrap();
        assert!(!lease_id.is_empty());

        // Get
        let retrieved = manager.get(node_id, &lease_id).unwrap();
        assert_eq!(retrieved, token);

        // Rotate
        let new_token = "sbp_new_rotated_token_987654";
        let new_lease_id = manager
            .rotate(node_id, provider, new_token, Some(&lease_id), 3600)
            .unwrap();

        // Old lease must now fail
        assert!(manager.get(node_id, &lease_id).is_err());

        // New lease must return new token
        let retrieved_new = manager.get(node_id, &new_lease_id).unwrap();
        assert_eq!(retrieved_new, new_token);

        // Revoke
        manager
            .revoke(
                node_id,
                provider,
                Some(&new_lease_id),
                "Test deprovision",
                true,
            )
            .unwrap();

        assert!(manager.get(node_id, &new_lease_id).is_err());
    }
}
