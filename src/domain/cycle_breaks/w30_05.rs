//! Wave 30 / ADR-033 Wave 0, issue WAVE-30.05: breaks the `crypto` / `mesh` /
//! `node_identity` / `governance` cycles. `NodeId` and `DerivedNodeKeys` moved
//! here from `mesh::node` and `node_identity::derive` (both still re-export
//! them); the capabilities invert the edges that cannot be moved.

use std::fmt;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The human-shareable identifier for a Xavier node (owned by `w30_04`).
pub use super::w30_04::NodeId;

impl NodeId {
    /// Parse a NodeID from a string. Validates the `xv1-` prefix and length.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        if !s.starts_with("xv1-") {
            anyhow::bail!("Invalid NodeID: must start with 'xv1-'. Got: {}", s);
        }
        if s.len() < 10 {
            anyhow::bail!("Invalid NodeID: too short ({})", s.len());
        }
        Ok(NodeId(s.to_string()))
    }

    /// Returns the raw string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Derive a NodeID from an Ed25519 public key bytes.
    pub fn from_public_key_bytes(pk_bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(pk_bytes);
        // First 15 bytes → base32 without padding → "xv1-" prefix.
        let encoded = base32_encode(&hasher.finalize()[..15]);
        NodeId(format!("xv1-{}", encoded.to_lowercase()))
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.0)
    }
}

/// Encode bytes as lowercase base32 without padding (Crockford-like).
fn base32_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let mut output = String::new();

    let mut buffer: u64 = 0;
    let mut bits_in_buffer: u32 = 0;

    for &byte in input {
        buffer = (buffer << 8) | (byte as u64);
        bits_in_buffer += 8;
        while bits_in_buffer >= 5 {
            bits_in_buffer -= 5;
            let idx = ((buffer >> bits_in_buffer) & 0x1F) as usize;
            output.push(ALPHABET[idx] as char);
        }
    }

    if bits_in_buffer > 0 {
        let idx = ((buffer << (5 - bits_in_buffer)) & 0x1F) as usize;
        output.push(ALPHABET[idx] as char);
    }

    output
}

/// Keys derived from BIP39 seed bytes (64-byte BIP39 seed).
#[derive(Clone)]
pub struct DerivedNodeKeys {
    pub node_id: NodeId,
    pub ed25519_public: [u8; 32],
    /// Signing key bytes — sensitive.
    pub ed25519_secret: [u8; 32],
    /// 32-byte commitment / seed for ML-DSA-65 keygen in edge-mesh (not a full PQ keypair).
    pub ml_dsa_commitment: [u8; 32],
}

/// Read-only mesh identity view, implemented by `mesh::node::NodeIdentity`.
pub trait NodeIdentityView {
    fn private_key_bytes(&self) -> &[u8];
    fn ml_dsa_commitment_hex(&self) -> Option<String>;
}

/// Builds a mesh node identity from pure keys (login F0 → F1 bridge).
pub trait NodeIdentityFactory: Sized {
    fn from_derived_keys(keys: &DerivedNodeKeys) -> Self;
}

/// Sealed-vault key material, implemented by `node_identity::NodeStore`.
pub trait SwalVaultKeySource {
    /// `Ok(None)` when the node has no sealed vault; `Err` when the vault exists
    /// but the PIN / device key does not unlock it.
    fn unlock_derived_keys(
        &self,
        pin: &str,
        device_key: Option<&[u8; 32]>,
    ) -> Result<Option<DerivedNodeKeys>, String>;
}

/// On-chain DAO ops, implemented by `mesh::governance::onchain::OnchainDaoClient`.
#[async_trait]
pub trait OnchainDaoGateway: Send + Sync {
    async fn propose(
        &self,
        proposal_id: &str,
        title: &str,
        description: &str,
    ) -> anyhow::Result<()>;
    async fn vote(
        &self,
        proposal_id: &str,
        approve: bool,
        voting_power: u64,
        is_council: bool,
    ) -> anyhow::Result<()>;
    async fn veto(&self, proposal_id: &str, reason: &str) -> anyhow::Result<()>;
    async fn overrule(&self, proposal_id: &str) -> anyhow::Result<()>;
    async fn execute(&self, proposal_id: &str) -> anyhow::Result<()>;
    async fn get_proposal_status(
        &self,
        proposal_id: &str,
    ) -> anyhow::Result<(bool, u64, u64, u64, u64, bool, bool)>;
}

/// Turns a deployment config into a live gateway (impl next to `EvmDaoConfig`).
pub trait IntoOnchainDaoGateway {
    fn into_onchain_dao_gateway(self) -> Box<dyn OnchainDaoGateway>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoVault;

    impl SwalVaultKeySource for NoVault {
        fn unlock_derived_keys(
            &self,
            _pin: &str,
            _device_key: Option<&[u8; 32]>,
        ) -> Result<Option<DerivedNodeKeys>, String> {
            Ok(None)
        }
    }

    struct RecordingDao {
        votes: std::sync::Mutex<Vec<(String, bool, u64, bool)>>,
    }

    #[async_trait]
    impl OnchainDaoGateway for RecordingDao {
        async fn propose(&self, _p: &str, _t: &str, _d: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn vote(
            &self,
            proposal_id: &str,
            approve: bool,
            voting_power: u64,
            is_council: bool,
        ) -> anyhow::Result<()> {
            let mut votes = self.votes.lock().unwrap();
            votes.push((proposal_id.to_string(), approve, voting_power, is_council));
            Ok(())
        }

        async fn veto(&self, _proposal_id: &str, _reason: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn overrule(&self, _proposal_id: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn execute(&self, _proposal_id: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn get_proposal_status(
            &self,
            _proposal_id: &str,
        ) -> anyhow::Result<(bool, u64, u64, u64, u64, bool, bool)> {
            Ok((true, 1, 2, 3, 4, false, false))
        }
    }

    #[test]
    fn node_id_is_deterministic_and_prefixed() {
        let pk = [0xAB_u8; 32];
        let id = NodeId::from_public_key_bytes(&pk);
        assert_eq!(id, NodeId::from_public_key_bytes(&pk));
        assert!(id.as_str().starts_with("xv1-"));
        assert!(id.as_str().len() >= 10);
        assert_ne!(id, NodeId::from_public_key_bytes(&[0x01_u8; 32]));
    }

    #[test]
    fn node_id_parse_validates_prefix() {
        let parsed = NodeId::parse("xv1-abc123defgh").unwrap();
        assert_eq!(parsed.as_str(), "xv1-abc123defgh");
        assert!(NodeId::parse("node-abc123").is_err());
        assert!(NodeId::parse("xv1-short").is_err());
    }

    #[test]
    fn vault_contract_is_implementable_outside_node_identity() {
        let unlocked = NoVault.unlock_derived_keys("1234", None).unwrap();
        assert!(unlocked.is_none());
    }

    #[tokio::test]
    async fn onchain_gateway_contract_is_implementable_outside_mesh() {
        let dao = RecordingDao {
            votes: std::sync::Mutex::new(Vec::new()),
        };
        dao.vote("xip-1", true, 100, false).await.unwrap();
        let status = dao.get_proposal_status("xip-1").await.unwrap();
        assert_eq!(status, (true, 1, 2, 3, 4, false, false));
        assert_eq!(dao.votes.lock().unwrap().len(), 1);
    }
}
