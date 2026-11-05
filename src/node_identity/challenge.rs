//! Ed25519 signature challenge authentication for cloud mesh sync.
//!
//! Provides cryptographic challenge signing and verification to protect
//! Xavier cloud mesh synchronization requests against token compromise.

use crate::mesh::node::{NodeId, NodeIdentity};
use anyhow::{bail, Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default maximum allowable clock skew in seconds (5 minutes).
pub const DEFAULT_MAX_SKEW_SECS: u64 = 300;

/// Canonical domain separation prefix for cloud sync signatures.
pub const CLOUD_SYNC_DOMAIN: &str = "xavier-cloud-sync-v1";

/// HTTP signature headers produced by signing a sync request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignatureHeader {
    /// The Node ID of the signer (e.g. "xv1-...")
    pub node_id: String,
    /// Hex-encoded Ed25519 public key (32 bytes)
    pub public_key_hex: String,
    /// Hex-encoded Ed25519 signature (64 bytes)
    pub signature_hex: String,
    /// Unix timestamp in seconds when the signature was created
    pub timestamp: u64,
    /// Cryptographic random nonce (hex-encoded 32 bytes)
    pub nonce: String,
}

impl SignatureHeader {
    /// Return the HTTP headers as key-value pairs.
    pub fn to_headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("X-Node-Id", self.node_id.clone()),
            ("X-Node-Public-Key", self.public_key_hex.clone()),
            ("X-Node-Signature", self.signature_hex.clone()),
            ("X-Timestamp", self.timestamp.to_string()),
            ("X-Nonce", self.nonce.clone()),
        ]
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Compute the canonical bytes to sign for a cloud sync request.
pub fn canonical_sign_payload(
    node_id: &str,
    payload_hash: &str,
    timestamp: u64,
    nonce: &str,
) -> Vec<u8> {
    format!("{CLOUD_SYNC_DOMAIN}|{node_id}|{payload_hash}|{timestamp}|{nonce}").into_bytes()
}

/// Sign a cloud mesh sync request with the node's local Ed25519 private key.
pub fn sign_sync_request(node_identity: &NodeIdentity, payload_hash: &str) -> SignatureHeader {
    let mut nonce_bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = crate::crypto::hex_encode(nonce_bytes);
    let timestamp = now_secs();

    let node_id = node_identity.node_id.as_str().to_string();
    let payload = canonical_sign_payload(&node_id, payload_hash, timestamp, &nonce);
    let signature = node_identity.sign(&payload);

    SignatureHeader {
        node_id,
        public_key_hex: crate::crypto::hex_encode(&node_identity.public_key),
        signature_hex: crate::crypto::hex_encode(signature),
        timestamp,
        nonce,
    }
}

/// Verify a cloud mesh sync request signature against the expected payload hash and maximum clock skew.
pub fn verify_sync_request(
    header: &SignatureHeader,
    payload_hash: &str,
    max_skew_secs: Option<u64>,
) -> Result<NodeId> {
    let now = now_secs();
    let skew_limit = max_skew_secs.unwrap_or(DEFAULT_MAX_SKEW_SECS);

    // Timestamp expiration / freshness check
    let diff = now.abs_diff(header.timestamp);

    if diff > skew_limit {
        bail!(
            "sync request expired: timestamp delta {}s exceeds max allowed {}s",
            diff,
            skew_limit
        );
    }

    let node_id = NodeId::parse(&header.node_id).context("invalid node_id format")?;
    let pk_bytes =
        crate::crypto::hex_decode(&header.public_key_hex).context("invalid public_key_hex")?;

    let expected_node_id = NodeId::from_public_key_bytes(&pk_bytes);
    if expected_node_id != node_id {
        bail!("node_id does not match public key in signature header");
    }

    let sig_bytes =
        crate::crypto::hex_decode(&header.signature_hex).context("invalid signature_hex")?;
    let payload = canonical_sign_payload(
        &header.node_id,
        payload_hash,
        header.timestamp,
        &header.nonce,
    );

    if !NodeIdentity::verify(&pk_bytes, &payload, &sig_bytes) {
        bail!("invalid Ed25519 signature on cloud sync request");
    }

    Ok(node_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_roundtrip_valid() {
        let identity = NodeIdentity::generate();
        let payload_hash = "a1b2c3d4e5f67890abcdef1234567890abcdef1234567890abcdef1234567890";

        let header = sign_sync_request(&identity, payload_hash);
        assert_eq!(header.node_id, identity.node_id.as_str());
        assert_eq!(
            header.public_key_hex,
            crate::crypto::hex_encode(&identity.public_key)
        );

        let verified = verify_sync_request(&header, payload_hash, None).unwrap();
        assert_eq!(verified, identity.node_id);

        let headers = header.to_headers();
        assert!(headers
            .iter()
            .any(|(k, v)| *k == "X-Node-Id" && v == header.node_id.as_str()));
        assert!(headers.iter().any(|(k, _)| *k == "X-Node-Signature"));
        assert!(headers.iter().any(|(k, _)| *k == "X-Timestamp"));
    }

    #[test]
    fn test_expired_timestamp_rejected() {
        let identity = NodeIdentity::generate();
        let payload_hash = "hash123";

        let mut header = sign_sync_request(&identity, payload_hash);
        // Expire timestamp beyond 300 seconds
        header.timestamp = now_secs().saturating_sub(600);

        let err = verify_sync_request(&header, payload_hash, Some(300)).unwrap_err();
        assert!(err.to_string().contains("expired"), "Error was: {}", err);
    }

    #[test]
    fn test_payload_hash_tampered_rejected() {
        let identity = NodeIdentity::generate();
        let original_hash = "good_hash";
        let tampered_hash = "bad_hash";

        let header = sign_sync_request(&identity, original_hash);
        let err = verify_sync_request(&header, tampered_hash, None).unwrap_err();
        assert!(
            err.to_string().contains("invalid Ed25519 signature"),
            "Error was: {}",
            err
        );
    }

    #[test]
    fn test_forged_public_key_rejected() {
        let identity_a = NodeIdentity::generate();
        let identity_b = NodeIdentity::generate();
        let payload_hash = "hash123";

        let mut header = sign_sync_request(&identity_a, payload_hash);
        header.public_key_hex = crate::crypto::hex_encode(&identity_b.public_key);
        header.node_id = identity_b.node_id.as_str().to_string();

        let err = verify_sync_request(&header, payload_hash, None).unwrap_err();
        assert!(
            err.to_string().contains("invalid Ed25519 signature"),
            "Error was: {}",
            err
        );
    }

    #[test]
    fn test_node_id_mismatch_rejected() {
        let identity = NodeIdentity::generate();
        let other = NodeIdentity::generate();
        let payload_hash = "hash123";

        let mut header = sign_sync_request(&identity, payload_hash);
        header.node_id = other.node_id.as_str().to_string();

        let err = verify_sync_request(&header, payload_hash, None).unwrap_err();
        assert!(
            err.to_string().contains("does not match public key"),
            "Error was: {}",
            err
        );
    }
}
