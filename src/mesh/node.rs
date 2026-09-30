//! Node Identity — Ed25519 keypair and NodeID
//!
//! Each Xavier node generates a persistent Ed25519 keypair on first startup.
//! The **NodeID** is a human-readable, base32-encoded truncation of the
//! BLAKE3 hash of the public key. It uniquely identifies a node regardless
//! of its IP address or network location.
//!
//! # NodeID Format
//!
//! ```text
//! NodeID = base32_lower(sha256(ed25519_public_key)[0..20])
//! Example: "xv1-abc2defg3hi4jklm5nop6qrs7"  (28 chars, URL-safe)
//! ```
//!
//! # Storage
//!
//! The keypair is stored in the system keyring (via `keyring` crate) under
//! the service name `xavier-mesh`. On first call to [`NodeIdentity::load_or_create`],
//! a new keypair is generated and persisted. Subsequent calls load the same identity.

use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::domain::cycle_breaks::w30_05::{
    DerivedNodeKeys, NodeIdentityFactory, NodeIdentityView, SwalVaultKeySource,
};

/// The human-shareable identifier for a Xavier node (domain type, wave 30).
pub use crate::domain::cycle_breaks::w30_05::NodeId;

// ---------------------------------------------------------------------------
// NodeIdentity — The full keypair + derived NodeID
// ---------------------------------------------------------------------------

/// The complete identity of a Xavier Mesh node.
///
/// Contains the Ed25519 keypair and the human-readable NodeID. The private
/// key is kept in memory and optionally persisted to the system keyring.
#[derive(Clone)]
pub struct NodeIdentity {
    /// Human-readable node identifier (from public key)
    pub node_id: NodeId,
    /// Ed25519 public key (32 bytes)
    pub public_key: Vec<u8>,
    /// Ed25519 private key (32 bytes) — sensitive, not serialized
    private_key: Vec<u8>,
    /// Optional ML-DSA commitment (32 bytes) from SWAL vault derive (login F0/F1).
    ml_dsa_commitment: Option<[u8; 32]>,
}

impl fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeIdentity")
            .field("node_id", &self.node_id)
            .field("public_key", &crate::crypto::hex_encode(&self.public_key))
            .field("private_key", &"[REDACTED]")
            .field(
                "ml_dsa_commitment",
                &self
                    .ml_dsa_commitment
                    .as_ref()
                    .map(crate::crypto::hex_encode),
            )
            .finish()
    }
}

impl NodeIdentity {
    /// Generate a brand new random Ed25519 keypair and derive a NodeID.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let public_key = signing_key.verifying_key();
        let pk_bytes = public_key.to_bytes();

        let node_id = NodeId::from_public_key_bytes(&pk_bytes);

        NodeIdentity {
            node_id,
            public_key: pk_bytes.to_vec(),
            private_key: signing_key.to_bytes().to_vec(),
            ml_dsa_commitment: None,
        }
    }

    /// Build mesh identity from SWAL vault-derived keys (login F0 → F1 bridge).
    pub fn from_derived(keys: &DerivedNodeKeys) -> Self {
        Self {
            node_id: keys.node_id.clone(),
            public_key: keys.ed25519_public.to_vec(),
            private_key: keys.ed25519_secret.to_vec(),
            ml_dsa_commitment: Some(keys.ml_dsa_commitment),
        }
    }

    /// Construct NodeIdentity from raw Ed25519 secret key seed bytes (32 bytes).
    pub fn from_private_key_bytes(bytes: &[u8]) -> Result<Self> {
        let key_bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Ed25519 private key must be exactly 32 bytes"))?;
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let public_key = signing_key.verifying_key();
        let pk_bytes = public_key.to_bytes();
        let node_id = NodeId::from_public_key_bytes(&pk_bytes);

        Ok(NodeIdentity {
            node_id,
            public_key: pk_bytes.to_vec(),
            private_key: bytes.to_vec(),
            ml_dsa_commitment: None,
        })
    }

    /// Prefer the injected [`SwalVaultKeySource`] vault; else load/create the keypair.
    pub fn load_preferring_swal_vault<V: SwalVaultKeySource>(
        pin: &str,
        device_key: Option<&[u8; 32]>,
        vault: &V,
    ) -> Result<Self> {
        match vault.unlock_derived_keys(pin, device_key) {
            Ok(Some(keys)) => Ok(Self::from_derived(&keys)),
            Ok(None) => Self::load_or_create(),
            Err(e) => Err(anyhow::anyhow!("swal vault unlock failed: {e}")),
        }
    }

    /// Hex ML-DSA commitment for hybrid / edge-mesh bridge (if present).
    pub fn ml_dsa_commitment_hex(&self) -> Option<String> {
        self.ml_dsa_commitment
            .as_ref()
            .map(crate::crypto::hex_encode)
    }

    /// Sign a message using the node's private key.
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let signing_key = SigningKey::from_bytes(
            self.private_key
                .as_slice()
                .try_into()
                .expect("invalid private key length"),
        );
        let signature: Signature = signing_key.sign(message);
        signature.to_bytes().to_vec()
    }

    /// Verify a signature against a public key.
    pub fn verify(public_key_bytes: &[u8], message: &[u8], signature_bytes: &[u8]) -> bool {
        let Ok(pk_arr) = <[u8; 32]>::try_from(public_key_bytes) else {
            return false;
        };
        let Ok(verifying_key) = VerifyingKey::from_bytes(&pk_arr) else {
            return false;
        };
        let Ok(sig_arr) = <[u8; 64]>::try_from(signature_bytes) else {
            return false;
        };
        let signature = Signature::from_bytes(&sig_arr);

        verifying_key.verify(message, &signature).is_ok()
    }

    /// Load existing identity from the system keyring, or generate a new one.
    ///
    /// This is the main entrypoint — call once at startup and cache the result.
    pub fn load_or_create() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("Could not determine config directory")?
            .join("xavier");

        std::fs::create_dir_all(&config_dir)?;

        let identity_file = config_dir.join("mesh_identity.json");

        if identity_file.exists() {
            let raw = std::fs::read_to_string(&identity_file)
                .context("Failed to read mesh identity file")?;
            let stored: StoredIdentity =
                serde_json::from_str(&raw).context("Failed to parse mesh identity file")?;
            return Self::from_stored(stored);
        }

        // First time: generate and persist
        let identity = Self::generate();
        let stored = StoredIdentity {
            version: 1,
            node_id: identity.node_id.0.clone(),
            public_key_hex: crate::crypto::hex_encode(&identity.public_key),
            private_key_hex: crate::crypto::hex_encode(&identity.private_key),
        };

        let json = serde_json::to_string_pretty(&stored)?;
        // Store with restrictive permissions on Linux/macOS via write + chmod
        std::fs::write(&identity_file, &json).context("Failed to write mesh identity file")?;

        // Attempt to restrict permissions (best-effort on Windows)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&identity_file, std::fs::Permissions::from_mode(0o600));
        }

        tracing::info!(
            node_id = %identity.node_id,
            "✨ Generated new Xavier Mesh identity"
        );

        Ok(identity)
    }

    fn from_stored(stored: StoredIdentity) -> Result<Self> {
        let public_key = crate::crypto::hex_decode(&stored.public_key_hex)
            .context("Invalid public key hex in identity file")?;
        let private_key = crate::crypto::hex_decode(&stored.private_key_hex)
            .context("Invalid private key hex in identity file")?;
        let node_id = NodeId(stored.node_id);

        Ok(NodeIdentity {
            node_id,
            public_key,
            private_key,
            ml_dsa_commitment: None,
        })
    }

    /// Return the private key bytes. Panics if used incorrectly — handle with care.
    pub fn private_key_bytes(&self) -> &[u8] {
        &self.private_key
    }

    /// Serialize the public identity for sharing with peers.
    pub fn public_info(&self) -> PublicNodeInfo {
        PublicNodeInfo {
            node_id: self.node_id.clone(),
            public_key_hex: crate::crypto::hex_encode(&self.public_key),
            xavier_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Public information about a node — safe to share with peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicNodeInfo {
    pub node_id: NodeId,
    pub public_key_hex: String,
    pub xavier_version: String,
}

/// Persisted form of the node identity (stored in config dir).
#[derive(Debug, Serialize, Deserialize)]
struct StoredIdentity {
    version: u32,
    node_id: String,
    public_key_hex: String,
    /// WARNING: private key stored in plaintext in config file.
    /// Phase 2 will move this to system keyring (keyring crate).
    private_key_hex: String,
}

// ---------------------------------------------------------------------------
// Domain capabilities (wave 30 / issue WAVE-30.05)
// ---------------------------------------------------------------------------

impl NodeIdentityView for NodeIdentity {
    fn private_key_bytes(&self) -> &[u8] {
        &self.private_key
    }

    fn ml_dsa_commitment_hex(&self) -> Option<String> {
        NodeIdentity::ml_dsa_commitment_hex(self)
    }
}

impl NodeIdentityFactory for NodeIdentity {
    fn from_derived_keys(keys: &DerivedNodeKeys) -> Self {
        NodeIdentity::from_derived(keys)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_generation_is_stable() {
        let identity = NodeIdentity::generate();
        let id_str = identity.node_id.as_str();
        assert!(id_str.starts_with("xv1-"), "NodeID must start with 'xv1-'");
        assert!(id_str.len() >= 10, "NodeID too short: {}", id_str);
    }

    #[test]
    fn test_node_id_deterministic() {
        let pk = [0xAB_u8; 32];
        let id1 = NodeId::from_public_key_bytes(&pk);
        let id2 = NodeId::from_public_key_bytes(&pk);
        assert_eq!(id1, id2, "Same public key should always yield same NodeID");
    }

    #[test]
    fn test_node_id_different_keys_different_ids() {
        let id1 = NodeId::from_public_key_bytes(&[0x01_u8; 32]);
        let id2 = NodeId::from_public_key_bytes(&[0x02_u8; 32]);
        assert_ne!(id1, id2, "Different keys must yield different NodeIDs");
    }

    #[test]
    fn test_node_id_parse_valid() {
        let id = NodeId::parse("xv1-abc123defgh").unwrap();
        assert_eq!(id.as_str(), "xv1-abc123defgh");
    }

    #[test]
    fn test_node_id_parse_invalid_prefix() {
        assert!(NodeId::parse("node-abc123").is_err());
        assert!(NodeId::parse("abc123").is_err());
    }

    #[test]
    fn test_two_identities_different() {
        let a = NodeIdentity::generate();
        let b = NodeIdentity::generate();
        assert_ne!(a.node_id, b.node_id, "Each identity must be unique");
    }

    struct StubVault {
        outcome: Result<Option<DerivedNodeKeys>, String>,
    }

    impl SwalVaultKeySource for StubVault {
        fn unlock_derived_keys(
            &self,
            _pin: &str,
            _device_key: Option<&[u8; 32]>,
        ) -> Result<Option<DerivedNodeKeys>, String> {
            match &self.outcome {
                Ok(Some(keys)) => Ok(Some(DerivedNodeKeys {
                    node_id: keys.node_id.clone(),
                    ed25519_public: keys.ed25519_public,
                    ed25519_secret: keys.ed25519_secret,
                    ml_dsa_commitment: keys.ml_dsa_commitment,
                })),
                Ok(None) => Ok(None),
                Err(e) => Err(e.clone()),
            }
        }
    }

    fn stub_keys() -> DerivedNodeKeys {
        let mut secret = [0u8; 32];
        secret[0] = 7;
        DerivedNodeKeys {
            node_id: NodeId::parse("xv1-stubvault00").unwrap(),
            ed25519_public: [1u8; 32],
            ed25519_secret: secret,
            ml_dsa_commitment: [2u8; 32],
        }
    }

    #[test]
    fn load_preferring_swal_vault_uses_injected_vault_keys() {
        let keys = stub_keys();
        let vault = StubVault {
            outcome: Ok(Some(keys.clone())),
        };
        let loaded = NodeIdentity::load_preferring_swal_vault("1234", None, &vault).unwrap();
        assert_eq!(loaded.node_id, keys.node_id);
        assert_eq!(loaded.private_key, keys.ed25519_secret.to_vec());
        assert_eq!(loaded.ml_dsa_commitment, Some(keys.ml_dsa_commitment));
    }

    #[test]
    fn load_preferring_swal_vault_propagates_unlock_failure() {
        let vault = StubVault {
            outcome: Err("bad pin".to_string()),
        };
        let err = NodeIdentity::load_preferring_swal_vault("0000", None, &vault).unwrap_err();
        assert_eq!(err.to_string(), "swal vault unlock failed: bad pin");
    }
}
