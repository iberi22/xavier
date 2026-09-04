//! SWAL Founder Node cryptographic attestation and genesis status.
//!
//! Provides Ed25519 cryptographic attestation of node metadata and genesis state,
//! along with verification helpers and HTTP status exposure.

use anyhow::Result;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use sha2::Digest;

use super::derive::DerivedNodeKeys;
use super::persist::NodeStore;
use crate::mesh::node::{NodeId, NodeIdentity};

/// SWAL Network genesis configuration parameters.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SwalGenesisParams {
    /// Epoch timestamp (in seconds) when the SWAL genesis block/network was initialized.
    pub genesis_timestamp: u64,
    /// Canonical network identifier string.
    pub network_id: String,
    /// Expected Ed25519 public key hex of the founder node (optional constraint).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_founder_public_key_hex: Option<String>,
}

impl Default for SwalGenesisParams {
    fn default() -> Self {
        Self {
            genesis_timestamp: 1704067200, // SWAL network genesis epoch (2024-01-01 00:00:00 UTC)
            network_id: "swal-mainnet-v1".to_string(),
            expected_founder_public_key_hex: None,
        }
    }
}

/// Metadata describing the node context in the SWAL ecosystem.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeMetadata {
    /// Version of Xavier / SWAL core running on this node.
    pub xavier_version: String,
    /// Role of this node in the network (e.g. "founder", "validator", "edge").
    pub role: String,
    /// Targeted SWAL network identifier.
    pub network_id: String,
}

impl Default for NodeMetadata {
    fn default() -> Self {
        Self {
            xavier_version: env!("CARGO_PKG_VERSION").to_string(),
            role: "founder".to_string(),
            network_id: "swal-mainnet-v1".to_string(),
        }
    }
}

/// Cryptographic attestation issued by the SWAL Founder Node.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FounderNodeAttestation {
    /// Schema version for attestation structure.
    pub version: u8,
    /// Human-readable NodeID derived from Ed25519 public key.
    pub node_id: String,
    /// Hex-encoded Ed25519 public key.
    pub ed25519_public_hex: String,
    /// SWAL genesis timestamp.
    pub genesis_timestamp: u64,
    /// Attestation timestamp (in seconds).
    pub timestamp: u64,
    /// Associated node metadata.
    pub node_metadata: NodeMetadata,
    /// Hex-encoded Ed25519 signature of canonical payload.
    pub signature_hex: String,
}

impl FounderNodeAttestation {
    /// Constructs the canonical byte payload to sign or verify.
    pub fn canonical_payload(
        node_id: &str,
        ed25519_public_hex: &str,
        genesis_timestamp: u64,
        timestamp: u64,
        node_metadata: &NodeMetadata,
    ) -> Vec<u8> {
        format!(
            "swal-founder-attestation-v1:{}:{}:{}:{}:{}:{}:{}",
            node_id,
            ed25519_public_hex,
            genesis_timestamp,
            timestamp,
            node_metadata.xavier_version,
            node_metadata.role,
            node_metadata.network_id
        )
        .into_bytes()
    }

    /// Helper to produce canonical bytes for this attestation instance.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        Self::canonical_payload(
            &self.node_id,
            &self.ed25519_public_hex,
            self.genesis_timestamp,
            self.timestamp,
            &self.node_metadata,
        )
    }

    /// Generate a new signed `FounderNodeAttestation` using node derived keys.
    pub fn generate(
        keys: &DerivedNodeKeys,
        genesis_timestamp: u64,
        timestamp: u64,
        node_metadata: NodeMetadata,
    ) -> Result<Self> {
        let node_id = keys.node_id.as_str().to_string();
        let ed25519_public_hex = crate::crypto::hex_encode(keys.ed25519_public);

        let payload = Self::canonical_payload(
            &node_id,
            &ed25519_public_hex,
            genesis_timestamp,
            timestamp,
            &node_metadata,
        );

        let sig_bytes = keys.sign(&payload);
        let signature_hex = crate::crypto::hex_encode(sig_bytes);

        Ok(Self {
            version: 1,
            node_id,
            ed25519_public_hex,
            genesis_timestamp,
            timestamp,
            node_metadata,
            signature_hex,
        })
    }
}

/// Helper function to generate a founder node cryptographic attestation.
pub fn generate_founder_attestation(
    keys: &DerivedNodeKeys,
    genesis_timestamp: u64,
    timestamp: u64,
    node_metadata: NodeMetadata,
) -> Result<FounderNodeAttestation> {
    FounderNodeAttestation::generate(keys, genesis_timestamp, timestamp, node_metadata)
}

/// Verification helper validating attestation signature and SWAL genesis parameters.
pub fn verify_founder_attestation(
    attestation: &FounderNodeAttestation,
    genesis_params: &SwalGenesisParams,
) -> Result<bool> {
    if attestation.genesis_timestamp != genesis_params.genesis_timestamp {
        tracing::warn!(
            "Attestation genesis timestamp mismatch: {} != {}",
            attestation.genesis_timestamp,
            genesis_params.genesis_timestamp
        );
        return Ok(false);
    }

    if attestation.node_metadata.network_id != genesis_params.network_id {
        tracing::warn!(
            "Attestation network_id mismatch: {} != {}",
            attestation.node_metadata.network_id,
            genesis_params.network_id
        );
        return Ok(false);
    }

    if let Some(ref expected_pk) = genesis_params.expected_founder_public_key_hex {
        if !attestation
            .ed25519_public_hex
            .eq_ignore_ascii_case(expected_pk)
        {
            tracing::warn!(
                "Founder public key mismatch: {} != {}",
                attestation.ed25519_public_hex,
                expected_pk
            );
            return Ok(false);
        }
    }

    if attestation.timestamp < attestation.genesis_timestamp {
        tracing::warn!(
            "Attestation timestamp prior to genesis: {} < {}",
            attestation.timestamp,
            attestation.genesis_timestamp
        );
        return Ok(false);
    }

    let public_key_bytes = match crate::crypto::hex_decode(&attestation.ed25519_public_hex) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(false),
    };

    let expected_node_id = NodeId::from_public_key_bytes(&public_key_bytes);
    if attestation.node_id != expected_node_id.as_str() {
        tracing::warn!(
            "Node ID does not match public key: {} != {}",
            attestation.node_id,
            expected_node_id.as_str()
        );
        return Ok(false);
    }

    let signature_bytes = match crate::crypto::hex_decode(&attestation.signature_hex) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(false),
    };

    let canonical_payload = attestation.to_canonical_bytes();
    let is_valid = NodeIdentity::verify(&public_key_bytes, &canonical_payload, &signature_bytes);

    Ok(is_valid)
}

/// Response format for `GET /node/founder/status`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FounderStatusResponse {
    pub status: String,
    pub is_valid: bool,
    pub attestation: FounderNodeAttestation,
    pub genesis_params: SwalGenesisParams,
}

/// HTTP Handler exposing attestation status (`GET /node/founder/status`).
pub async fn founder_status_handler() -> impl IntoResponse {
    let genesis_params = SwalGenesisParams::default();
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => genesis_params.genesis_timestamp,
    };

    let attestation_res = (|| -> Result<FounderNodeAttestation> {
        let store = NodeStore::default_from_env();
        if store.vault_exists() {
            if let Ok((_opened, keys, _codes)) = store.unlock("", None) {
                return generate_founder_attestation(
                    &keys,
                    genesis_params.genesis_timestamp,
                    now,
                    NodeMetadata::default(),
                );
            }
        }

        let identity = NodeIdentity::load_or_create()?;
        let seed = {
            let mut hasher = sha2::Sha256::new();
            hasher.update(identity.private_key_bytes());
            hasher.update(b"swal-founder-fallback");
            let mut seed = [0u8; 64];
            let hash = hasher.finalize();
            seed[..32].copy_from_slice(&hash);
            seed[32..].copy_from_slice(&hash);
            seed
        };
        let keys = DerivedNodeKeys::from_seed_bytes(&seed)?;
        generate_founder_attestation(
            &keys,
            genesis_params.genesis_timestamp,
            now,
            NodeMetadata::default(),
        )
    })();

    match attestation_res {
        Ok(attestation) => {
            let is_valid =
                verify_founder_attestation(&attestation, &genesis_params).unwrap_or(false);
            let status = if is_valid { "active" } else { "degraded" };

            axum::Json(FounderStatusResponse {
                status: status.to_string(),
                is_valid,
                attestation,
                genesis_params,
            })
            .into_response()
        }
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({
                "status": "error",
                "message": format!("Failed to generate founder attestation: {err}")
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attestation_generation_and_verification_roundtrip() {
        let seed = [7u8; 64];
        let keys = DerivedNodeKeys::from_seed_bytes(&seed).unwrap();
        let genesis_params = SwalGenesisParams::default();

        let attestation = generate_founder_attestation(
            &keys,
            genesis_params.genesis_timestamp,
            genesis_params.genesis_timestamp + 3600,
            NodeMetadata::default(),
        )
        .unwrap();

        assert_eq!(attestation.version, 1);
        assert_eq!(attestation.node_id, keys.node_id.as_str());

        let is_valid = verify_founder_attestation(&attestation, &genesis_params).unwrap();
        assert!(is_valid);
    }

    #[test]
    fn attestation_tampered_signature_fails() {
        let seed = [7u8; 64];
        let keys = DerivedNodeKeys::from_seed_bytes(&seed).unwrap();
        let genesis_params = SwalGenesisParams::default();

        let mut attestation = generate_founder_attestation(
            &keys,
            genesis_params.genesis_timestamp,
            genesis_params.genesis_timestamp + 3600,
            NodeMetadata::default(),
        )
        .unwrap();

        // Alter signature
        attestation.signature_hex = "00".repeat(64);
        let is_valid = verify_founder_attestation(&attestation, &genesis_params).unwrap();
        assert!(!is_valid);
    }

    #[test]
    fn attestation_mismatched_genesis_params_fails() {
        let seed = [7u8; 64];
        let keys = DerivedNodeKeys::from_seed_bytes(&seed).unwrap();
        let genesis_params = SwalGenesisParams::default();

        let attestation = generate_founder_attestation(
            &keys,
            genesis_params.genesis_timestamp,
            genesis_params.genesis_timestamp + 3600,
            NodeMetadata::default(),
        )
        .unwrap();

        let wrong_params = SwalGenesisParams {
            genesis_timestamp: 1000000,
            network_id: "swal-mainnet-v1".to_string(),
            expected_founder_public_key_hex: None,
        };

        let is_valid = verify_founder_attestation(&attestation, &wrong_params).unwrap();
        assert!(!is_valid);
    }

    #[test]
    fn attestation_expected_public_key_constraint() {
        let seed = [7u8; 64];
        let keys = DerivedNodeKeys::from_seed_bytes(&seed).unwrap();
        let pk_hex = crate::crypto::hex_encode(keys.ed25519_public);

        let matching_params = SwalGenesisParams {
            genesis_timestamp: 1704067200,
            network_id: "swal-mainnet-v1".to_string(),
            expected_founder_public_key_hex: Some(pk_hex.clone()),
        };

        let attestation = generate_founder_attestation(
            &keys,
            matching_params.genesis_timestamp,
            matching_params.genesis_timestamp + 10,
            NodeMetadata::default(),
        )
        .unwrap();

        assert!(verify_founder_attestation(&attestation, &matching_params).unwrap());

        let non_matching_params = SwalGenesisParams {
            genesis_timestamp: 1704067200,
            network_id: "swal-mainnet-v1".to_string(),
            expected_founder_public_key_hex: Some("ff".repeat(32)),
        };

        assert!(!verify_founder_attestation(&attestation, &non_matching_params).unwrap());
    }
}
