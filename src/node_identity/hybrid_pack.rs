//! Hybrid sealed-pack signatures — Ed25519 + ML-DSA commitment (Fase 3 / DL-F3-01).
//!
//! Full ML-DSA-65 signatures live in edge-mesh. Xavier stores/verifies the Ed25519
//! half and attaches the public ML-DSA commitment so verifiers can re-derive /
//! check the PQ half via `edge-mesh` `xavier-bridge`.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::mesh::node::NodeIdentity;
use crate::polygon_anchor::sealed_pack_content_hash;

/// Domain-separated message for pack authentication.
pub fn pack_sign_payload(content_hash_hex: &str, meta_utf8: &str) -> Vec<u8> {
    format!("swal-hybrid-pack-v1|{content_hash_hex}|{meta_utf8}").into_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HybridPackSignature {
    pub version: u8,
    pub content_hash_hex: String,
    pub ed25519_public_hex: String,
    pub ed25519_signature_hex: String,
    /// Public ML-DSA commitment from vault (not a full PQ signature blob).
    pub ml_dsa_commitment_hex: Option<String>,
    /// Optional PQ signature hex produced by edge-mesh (opaque to Xavier).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ml_dsa_signature_hex: Option<String>,
}

impl HybridPackSignature {
    /// Sign pack content with Ed25519 node key; attach ML-DSA commitment if present.
    pub fn sign_ed25519(
        identity: &NodeIdentity,
        ciphertext: &[u8],
        meta_utf8: &str,
    ) -> Result<Self> {
        let content_hash_hex = sealed_pack_content_hash(ciphertext, meta_utf8);
        let payload = pack_sign_payload(&content_hash_hex, meta_utf8);
        let sig = identity.sign(&payload);
        Ok(Self {
            version: 1,
            content_hash_hex,
            ed25519_public_hex: crate::crypto::hex_encode(&identity.public_key),
            ed25519_signature_hex: crate::crypto::hex_encode(sig),
            ml_dsa_commitment_hex: identity.ml_dsa_commitment_hex(),
            ml_dsa_signature_hex: None,
        })
    }

    /// Verify Ed25519 half (PQ half verified in edge-mesh when signature present).
    pub fn verify_ed25519(&self, meta_utf8: &str) -> Result<()> {
        let payload = pack_sign_payload(&self.content_hash_hex, meta_utf8);
        let pk = crate::crypto::hex_decode(&self.ed25519_public_hex)?;
        let sig = crate::crypto::hex_decode(&self.ed25519_signature_hex)?;
        if !NodeIdentity::verify(&pk, &payload, &sig) {
            bail!("invalid Ed25519 pack signature");
        }
        Ok(())
    }

    /// True when both classical signature and PQ commitment are present (hybrid ready).
    pub fn is_hybrid_ready(&self) -> bool {
        self.ml_dsa_commitment_hex
            .as_ref()
            .map(|s| s.len() == 64)
            .unwrap_or(false)
            && !self.ed25519_signature_hex.is_empty()
    }
}

/// Detached content-hash for on-chain anchor (never includes ciphertext).
pub fn onchain_pack_hash(ciphertext: &[u8], meta_utf8: &str) -> String {
    sealed_pack_content_hash(ciphertext, meta_utf8)
}

/// Cheap integrity tag for local audit logs (not a MAC secret).
pub fn audit_tag(content_hash_hex: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"swal-pack-audit-v1|");
    h.update(content_hash_hex.as_bytes());
    crate::crypto::hex_encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_identity::NodeBootstrap;

    #[test]
    fn hybrid_ed25519_roundtrip_with_commitment() {
        let bundle = NodeBootstrap::create(None, "packpin", None).unwrap();
        let id = NodeIdentity::from_derived(&bundle.keys);
        let cipher = b"ciphertext-not-on-chain";
        let meta = r#"{"v":1,"app":"swal"}"#;
        let sig = HybridPackSignature::sign_ed25519(&id, cipher, meta).unwrap();
        sig.verify_ed25519(meta).unwrap();
        assert!(sig.is_hybrid_ready());
        assert_eq!(sig.content_hash_hex, onchain_pack_hash(cipher, meta));
        assert!(!sig.content_hash_hex.is_empty());
    }

    #[test]
    fn forged_sig_rejected() {
        let a = NodeIdentity::generate();
        let b = NodeIdentity::generate();
        let sig = HybridPackSignature::sign_ed25519(&a, b"x", "{}").unwrap();
        let mut bad = sig.clone();
        bad.ed25519_public_hex = crate::crypto::hex_encode(&b.public_key);
        assert!(bad.verify_ed25519("{}").is_err());
    }
}
