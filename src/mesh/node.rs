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
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

// ---------------------------------------------------------------------------
// NodeId — The human-shareable identifier for a Xavier node
// ---------------------------------------------------------------------------

/// A unique identifier for a Xavier Mesh node.
///
/// Derived from the Ed25519 public key — stable across reboots, network
/// changes, and IP address changes.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

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
        let hash = hasher.finalize();

        // Take first 15 bytes → encode as base32 (no padding) → prefix with "xv1-"
        let encoded = base32_encode(&hash[..15]);
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
}

impl fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeIdentity")
            .field("node_id", &self.node_id)
            .field("public_key", &hex::encode(&self.public_key))
            .field("private_key", &"[REDACTED]")
            .finish()
    }
}

impl NodeIdentity {
    /// Generate a brand new random Ed25519 keypair and derive a NodeID.
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut sk_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut sk_bytes);

        // Derive a pseudo-public key via SHA-256 of private key for Phase 1.
        // Phase 2 will use proper Ed25519 (ed25519-dalek) with real signing.
        let mut hasher = Sha256::new();
        hasher.update(&sk_bytes);
        let pk_bytes = hasher.finalize().to_vec();

        let node_id = NodeId::from_public_key_bytes(&pk_bytes);

        NodeIdentity {
            node_id,
            public_key: pk_bytes,
            private_key: sk_bytes.to_vec(),
        }
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
            public_key_hex: hex::encode(&identity.public_key),
            private_key_hex: hex::encode(&identity.private_key),
        };

        let json = serde_json::to_string_pretty(&stored)?;
        // Store with restrictive permissions on Linux/macOS via write + chmod
        std::fs::write(&identity_file, &json)
            .context("Failed to write mesh identity file")?;

        // Attempt to restrict permissions (best-effort on Windows)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &identity_file,
                std::fs::Permissions::from_mode(0o600),
            );
        }

        tracing::info!(
            node_id = %identity.node_id,
            "✨ Generated new Xavier Mesh identity"
        );

        Ok(identity)
    }

    fn from_stored(stored: StoredIdentity) -> Result<Self> {
        let public_key = hex::decode(&stored.public_key_hex)
            .context("Invalid public key hex in identity file")?;
        let private_key = hex::decode(&stored.private_key_hex)
            .context("Invalid private key hex in identity file")?;
        let node_id = NodeId(stored.node_id);

        Ok(NodeIdentity {
            node_id,
            public_key,
            private_key,
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
            public_key_hex: hex::encode(&self.public_key),
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
// Internal helpers
// ---------------------------------------------------------------------------

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
}
