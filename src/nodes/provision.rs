//! Node provisioning engine and orchestration (Olas M6/M7, REQ-029/030)
//!
//! Enforces:
//! 1. Rejection of personal SSH keys (dedicated Ed25519 keypair generated per node).
//! 2. Token CLI flag `--token` allowed ONLY when `XAVIER_ALLOW_CLI_TOKEN=1`.
//! 3. Default visibility = `private`.
//! 4. Secrets persistence strictly in `src/secrets/` with `KeyLendingEngine` leases.
//! 5. Explicit `PartialRevocation` status if remote deprovisioning fails.

use crate::utils::crypto::hex_encode;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::io::{self, IsTerminal, Read};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::mesh::node::NodeId;
use crate::nodes::audit::{log_node_audit, NodeAuditEvent};
use crate::nodes::cert::{issue_cert, NodeCertificate};
use crate::nodes::registry::NodeRegistry;
use crate::nodes::secrets::NodeSecretsManager;
use crate::nodes::{NodeRecord, NodeStatus, NodeVisibility, Provider};

/// Deprovisioning outcome from a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeprovisionOutcome {
    Success,
    Partial { reason: String },
}

/// Request parameters for provisioning a node.
#[derive(Debug, Clone)]
pub struct ProvisionRequest {
    pub provider: Provider,
    pub visibility: NodeVisibility,
    pub token: Option<String>,
    pub ssh_host: Option<String>,
    pub host_key_fingerprint: Option<String>,
}

/// Response returned from a node provisioner.
#[derive(Debug, Clone)]
pub struct ProvisionResponse {
    pub provider_node_ref: String,
    pub details: String,
}

/// Trait defining the contract for node providers (BaaS / VPS).
///
/// Implemented by `MockProvisioner` for testing in this core wave,
/// and live adapters in subsequent waves (M6 Supabase/Neon, M7 edge-hive lite).
#[async_trait]
pub trait NodeProvisioner: Send + Sync {
    async fn provision(&self, req: &ProvisionRequest) -> Result<ProvisionResponse>;
    async fn rotate(&self, node_id: &str, new_token: &str) -> Result<()>;
    async fn deprovision(&self, node_id: &str) -> Result<DeprovisionOutcome>;
}

/// Mock provisioner for testing and unit validation.
pub struct MockProvisioner {
    should_fail_deprovision: bool,
    failure_reason: Option<String>,
}

impl Default for MockProvisioner {
    fn default() -> Self {
        Self::new()
    }
}

impl MockProvisioner {
    pub fn new() -> Self {
        Self {
            should_fail_deprovision: false,
            failure_reason: None,
        }
    }

    pub fn with_failing_deprovision(reason: impl Into<String>) -> Self {
        Self {
            should_fail_deprovision: true,
            failure_reason: Some(reason.into()),
        }
    }
}

#[async_trait]
impl NodeProvisioner for MockProvisioner {
    async fn provision(&self, req: &ProvisionRequest) -> Result<ProvisionResponse> {
        Ok(ProvisionResponse {
            provider_node_ref: format!("mock-{}-{}", req.provider, uuid::Uuid::new_v4()),
            details: format!("Mock provisioned {} successfully", req.provider),
        })
    }

    async fn rotate(&self, _node_id: &str, new_token: &str) -> Result<()> {
        if new_token.trim().is_empty() {
            return Err(anyhow!("Cannot rotate to an empty token"));
        }
        Ok(())
    }

    async fn deprovision(&self, _node_id: &str) -> Result<DeprovisionOutcome> {
        if self.should_fail_deprovision {
            Ok(DeprovisionOutcome::Partial {
                reason: self
                    .failure_reason
                    .clone()
                    .unwrap_or_else(|| "Remote provider API unreachable".to_string()),
            })
        } else {
            Ok(DeprovisionOutcome::Success)
        }
    }
}

// ---------------------------------------------------------------------------
// Security Validations
// ---------------------------------------------------------------------------

/// Validate and resolve token according to security rules.
///
/// Flag `--token` is ONLY permitted if `XAVIER_ALLOW_CLI_TOKEN=1`.
/// In production, read from `XAVIER_NODE_TOKEN` or stdin.
pub fn resolve_token(cli_token: Option<&str>) -> Result<String> {
    if let Some(tok) = cli_token {
        if !tok.trim().is_empty() {
            let allow_cli = std::env::var("XAVIER_ALLOW_CLI_TOKEN")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

            if !allow_cli {
                return Err(anyhow!(
                    "Security violation: The '--token' CLI flag is permitted ONLY in test environments (XAVIER_ALLOW_CLI_TOKEN=1).\n\
                     In production, supply the token via the 'XAVIER_NODE_TOKEN' environment variable or standard input."
                ));
            }
            return Ok(tok.trim().to_string());
        }
    }

    // Check environment variable
    if let Ok(env_tok) = std::env::var("XAVIER_NODE_TOKEN") {
        if !env_tok.trim().is_empty() {
            return Ok(env_tok.trim().to_string());
        }
    }

    // Try reading from stdin if available and not a terminal
    if !io::stdin().is_terminal() {
        let mut buffer = String::new();
        if io::stdin().read_to_string(&mut buffer).is_ok() && !buffer.trim().is_empty() {
            return Ok(buffer.trim().to_string());
        }
    }

    Err(anyhow!(
        "Missing node token. Set XAVIER_NODE_TOKEN environment variable or pipe via stdin."
    ))
}

/// Validate that no personal SSH key was passed.
///
/// Xavier MUST reject importing personal SSH keys to protect the user's host access.
pub fn validate_no_personal_ssh_key(personal_key_flag: Option<&str>) -> Result<()> {
    if let Some(key_path) = personal_key_flag {
        if !key_path.trim().is_empty() {
            return Err(anyhow!(
                "Security violation: Importing personal SSH keys ('{}') is strictly prohibited (REQ-030 / P0).\n\
                 Xavier automatically generates a dedicated Ed25519 keypair per node.",
                key_path
            ));
        }
    }
    Ok(())
}

/// Validate that a rotation token is not locally generated dummy junk.
pub fn validate_rotation_token(token: &str) -> Result<()> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Rotation requires a valid non-empty token"));
    }
    if trimmed.starts_with("clavis_") {
        return Err(anyhow!(
            "Invalid rotation token: locally generated clavis tokens ('{}') are invalid for external providers (ADR-015 P0).",
            trimmed
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Orchestration Logic
// ---------------------------------------------------------------------------

/// Provisioning engine coordinating secrets, certificates, registry, and provisioner.
pub struct ProvisioningEngine<P: NodeProvisioner> {
    registry: Arc<NodeRegistry>,
    secrets: NodeSecretsManager,
    provisioner: Arc<P>,
}

impl<P: NodeProvisioner> ProvisioningEngine<P> {
    pub fn new(
        registry: Arc<NodeRegistry>,
        secrets: NodeSecretsManager,
        provisioner: Arc<P>,
    ) -> Self {
        Self {
            registry,
            secrets,
            provisioner,
        }
    }

    /// Provision a new node (BaaS or SSH/VPS).
    #[allow(clippy::too_many_arguments)]
    pub async fn provision_node(
        &self,
        wallet_signing_key: &SigningKey,
        provider: Provider,
        visibility: NodeVisibility,
        token: Option<String>,
        ssh_host: Option<String>,
        host_key_fingerprint: Option<String>,
        cert_ttl_secs: u64,
        lease_ttl_secs: u64,
    ) -> Result<NodeRecord> {
        // 1. Generate dedicated Ed25519 keypair for node
        let node_signing_key = SigningKey::generate(&mut OsRng);
        let node_pubkey_bytes = node_signing_key.verifying_key().to_bytes();
        let node_pubkey_hex = hex_encode(&node_pubkey_bytes);

        // Derivation: node_id = hash(pubkey) with xv1- prefix
        let node_id = NodeId::from_public_key_bytes(&node_pubkey_bytes).0;

        // 2. Issue NodeCertificate signed by the wallet authority
        let cert = issue_cert(
            wallet_signing_key,
            &node_pubkey_bytes,
            &node_id,
            cert_ttl_secs,
        )
        .context("Failed to issue node certificate")?;

        // 3. Prepare secret credential to store
        let secret_to_store = match provider {
            Provider::Supabase | Provider::Neon => token
                .as_deref()
                .ok_or_else(|| anyhow!("Token required for BaaS provider {}", provider))?,
            Provider::Vps => {
                // For VPS, store the dedicated private key hex
                &hex_encode(&node_signing_key.to_bytes())
            }
        };

        // 4. Store secret in vault (AES-256-GCM) + create EphemeralLease
        let lease_id = self
            .secrets
            .store(&node_id, provider, secret_to_store, lease_ttl_secs)
            .context("Failed to store node secret in vault")?;

        // 5. Call provider provisioner
        let req = ProvisionRequest {
            provider,
            visibility,
            token: token.clone(),
            ssh_host: ssh_host.clone(),
            host_key_fingerprint: host_key_fingerprint.clone(),
        };
        let _provision_res = self
            .provisioner
            .provision(&req)
            .await
            .context("Provider provisioning failed")?;

        // 6. Record in persistent registry
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let record = NodeRecord {
            node_id: node_id.clone(),
            provider,
            visibility,
            status: NodeStatus::Active,
            pubkey: node_pubkey_hex,
            cert: Some(cert),
            host_key_fingerprint,
            lease_id: Some(lease_id),
            created_at: now,
            last_heartbeat: Some(now),
        };

        self.registry
            .register(&record)
            .context("Failed to persist node record in registry")?;

        log_node_audit(
            NodeAuditEvent::Provision,
            &node_id,
            provider,
            &format!("Node provisioned (Visibility: {})", visibility),
        );

        Ok(record)
    }

    /// Rotate credentials for an existing node.
    pub async fn rotate_node(
        &self,
        node_id: &str,
        new_token: &str,
        lease_ttl_secs: u64,
    ) -> Result<NodeRecord> {
        validate_rotation_token(new_token)?;

        let mut record = self
            .registry
            .get(node_id)?
            .ok_or_else(|| anyhow!("Node '{}' not found in registry", node_id))?;

        // 1. Tell provider to rotate credential
        self.provisioner
            .rotate(node_id, new_token)
            .await
            .context("Provider credential rotation failed")?;

        // 2. Rotate in secrets vault (revokes old lease, creates new lease)
        let new_lease = self
            .secrets
            .rotate(
                node_id,
                record.provider,
                new_token,
                record.lease_id.as_deref(),
                lease_ttl_secs,
            )
            .context("Failed to rotate secret in vault")?;

        record.lease_id = Some(new_lease);
        record.status = NodeStatus::Active;

        self.registry.register(&record)?;

        log_node_audit(
            NodeAuditEvent::Rotate,
            node_id,
            record.provider,
            "Node credentials rotated successfully",
        );

        Ok(record)
    }

    /// Remove a node and deprovision remote resources.
    ///
    /// If remote deprovisioning fails, explicitly records `PartialRevocation`.
    pub async fn remove_node(&self, node_id: &str) -> Result<NodeStatus> {
        let record = self
            .registry
            .get(node_id)?
            .ok_or_else(|| anyhow!("Node '{}' not found in registry", node_id))?;

        // 1. Revoke lease in secrets manager
        let _ = self.secrets.revoke(
            node_id,
            record.provider,
            record.lease_id.as_deref(),
            "Node removal",
            false,
        );

        // 2. Call deprovision on provider
        let outcome = self
            .provisioner
            .deprovision(node_id)
            .await
            .unwrap_or_else(|e| DeprovisionOutcome::Partial {
                reason: e.to_string(),
            });

        match outcome {
            DeprovisionOutcome::Success => {
                self.registry.update_status(node_id, NodeStatus::Revoked)?;
                log_node_audit(
                    NodeAuditEvent::Remove,
                    node_id,
                    record.provider,
                    "Node fully revoked and deprovisioned",
                );
                Ok(NodeStatus::Revoked)
            }
            DeprovisionOutcome::Partial { reason } => {
                self.registry
                    .update_status(node_id, NodeStatus::PartialRevocation)?;
                log_node_audit(
                    NodeAuditEvent::Remove,
                    node_id,
                    record.provider,
                    &format!("Node partial revocation: {}", reason),
                );
                Ok(NodeStatus::PartialRevocation)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_cli_token_without_env_flag() {
        std::env::remove_var("XAVIER_ALLOW_CLI_TOKEN");
        std::env::remove_var("XAVIER_NODE_TOKEN");

        let res = resolve_token(Some("sbp_forbidden_via_flag"));
        assert!(res.is_err(), "Must reject --token without allow flag");
        assert!(res.unwrap_err().to_string().contains("Security violation"));
    }

    #[test]
    fn test_allow_cli_token_with_env_flag() {
        std::env::set_var("XAVIER_ALLOW_CLI_TOKEN", "1");
        let res = resolve_token(Some("sbp_allowed_test_token"));
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), "sbp_allowed_test_token");
        std::env::remove_var("XAVIER_ALLOW_CLI_TOKEN");
    }

    #[test]
    fn test_read_token_from_env() {
        std::env::remove_var("XAVIER_ALLOW_CLI_TOKEN");
        std::env::set_var("XAVIER_NODE_TOKEN", "sbp_from_environment");

        let res = resolve_token(None);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), "sbp_from_environment");
        std::env::remove_var("XAVIER_NODE_TOKEN");
    }

    #[test]
    fn test_reject_personal_ssh_key() {
        let res = validate_no_personal_ssh_key(Some("~/.ssh/id_ed25519"));
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("strictly prohibited"));

        assert!(validate_no_personal_ssh_key(None).is_ok());
    }

    #[test]
    fn test_reject_clavis_dummy_token_rotation() {
        let res = validate_rotation_token("clavis_supabase_12345");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("locally generated"));
    }

    #[tokio::test]
    async fn test_provision_rotate_remove_lifecycle() {
        let registry = Arc::new(NodeRegistry::open_in_memory().unwrap());
        let secrets = NodeSecretsManager::new();
        let provisioner = Arc::new(MockProvisioner::new());

        let engine = ProvisioningEngine::new(registry.clone(), secrets, provisioner);
        let wallet_sk = SigningKey::generate(&mut OsRng);

        // Provision
        let record = engine
            .provision_node(
                &wallet_sk,
                Provider::Supabase,
                NodeVisibility::Private,
                Some("sbp_test_token".to_string()),
                None,
                None,
                3600,
                3600,
            )
            .await
            .unwrap();

        assert_eq!(record.provider, Provider::Supabase);
        assert_eq!(record.visibility, NodeVisibility::Private);
        assert_eq!(record.status, NodeStatus::Active);
        assert!(record.cert.is_some());

        // Rotate
        let rotated = engine
            .rotate_node(&record.node_id, "sbp_new_token_456", 3600)
            .await
            .unwrap();
        assert_ne!(record.lease_id, rotated.lease_id);

        // Remove
        let status = engine.remove_node(&record.node_id).await.unwrap();
        assert_eq!(status, NodeStatus::Revoked);

        let final_rec = registry.get(&record.node_id).unwrap().unwrap();
        assert_eq!(final_rec.status, NodeStatus::Revoked);
    }

    #[tokio::test]
    async fn test_deprovision_failure_yields_partial_revocation() {
        let registry = Arc::new(NodeRegistry::open_in_memory().unwrap());
        let secrets = NodeSecretsManager::new();
        let provisioner = Arc::new(MockProvisioner::with_failing_deprovision(
            "Supabase API error 500",
        ));

        let engine = ProvisioningEngine::new(registry.clone(), secrets, provisioner);
        let wallet_sk = SigningKey::generate(&mut OsRng);

        let record = engine
            .provision_node(
                &wallet_sk,
                Provider::Supabase,
                NodeVisibility::Private,
                Some("sbp_test_token".to_string()),
                None,
                None,
                3600,
                3600,
            )
            .await
            .unwrap();

        let status = engine.remove_node(&record.node_id).await.unwrap();
        assert_eq!(status, NodeStatus::PartialRevocation);

        let final_rec = registry.get(&record.node_id).unwrap().unwrap();
        assert_eq!(final_rec.status, NodeStatus::PartialRevocation);
    }
}
