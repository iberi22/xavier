//! HKDF domain-separated derivation: Ed25519 node key + ML-DSA commitment seed.

use anyhow::{anyhow, Result};
use ed25519_dalek::{SigningKey, VerifyingKey};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::mesh::node::NodeId;

/// Domain separation labels (LOGIN_IDENTITY_DESIGN §3.2).
pub const DOMAIN_NODE_ED25519: &[u8] = b"swal-node-ed25519-v1";
pub const DOMAIN_ML_DSA: &[u8] = b"swal-ml-dsa-65-seed-v1";

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

impl std::fmt::Debug for DerivedNodeKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DerivedNodeKeys")
            .field("node_id", &self.node_id)
            .field(
                "ed25519_public",
                &crate::crypto::hex_encode(self.ed25519_public),
            )
            .field("ed25519_secret", &"[REDACTED]")
            .field(
                "ml_dsa_commitment",
                &crate::crypto::hex_encode(self.ml_dsa_commitment),
            )
            .finish()
    }
}

impl DerivedNodeKeys {
    pub fn from_seed_bytes(seed_bytes: &[u8; 64]) -> Result<Self> {
        let ed_sk = hkdf_32(seed_bytes, DOMAIN_NODE_ED25519)?;
        let ml_commit = hkdf_32(seed_bytes, DOMAIN_ML_DSA)?;

        let signing = SigningKey::from_bytes(&ed_sk);
        let verifying: VerifyingKey = signing.verifying_key();
        let pk = verifying.to_bytes();
        let node_id = NodeId::from_public_key_bytes(&pk);

        Ok(Self {
            node_id,
            ed25519_public: pk,
            ed25519_secret: ed_sk,
            ml_dsa_commitment: ml_commit,
        })
    }

    /// Sign a message with the derived Ed25519 node key.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        use ed25519_dalek::Signer;
        let sk = SigningKey::from_bytes(&self.ed25519_secret);
        sk.sign(message).to_bytes()
    }
}

fn hkdf_32(ikm: &[u8], info: &[u8]) -> Result<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(None, ikm);
    let mut out = [0u8; 32];
    hk.expand(info, &mut out)
        .map_err(|_| anyhow!("HKDF expand failed"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_stable_and_distinct_domains() {
        let seed = [9u8; 64];
        let a = DerivedNodeKeys::from_seed_bytes(&seed).unwrap();
        let b = DerivedNodeKeys::from_seed_bytes(&seed).unwrap();
        assert_eq!(a.node_id, b.node_id);
        assert_eq!(a.ed25519_public, b.ed25519_public);
        assert_eq!(a.ml_dsa_commitment, b.ml_dsa_commitment);
        assert_ne!(a.ed25519_secret, a.ml_dsa_commitment);

        let msg = b"swal-challenge";
        let sig = a.sign(msg);
        assert!(crate::mesh::node::NodeIdentity::verify(
            &a.ed25519_public,
            msg,
            &sig
        ));
    }
}
