//! SWAL Fase 1 — signed nonce challenge-response (Ed25519) for mesh login.
//!
//! Mirrors edge-mesh `signed_nonce` semantics: prove possession of the private key
//! corresponding to a published `node_id` + Ed25519 public key. No "social trust"
//! shortcut. ML-DSA verification remains in edge-mesh; Xavier uses Ed25519 for
//! the HTTP/mesh data-plane path (LOGIN_IDENTITY_DESIGN §4.2 hybrid).

use crate::mesh::node::{NodeId, NodeIdentity};
use anyhow::{bail, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_TTL_SECS: u64 = 60;

/// Pending one-shot challenges keyed by challenge_id.
fn pending() -> &'static Mutex<HashMap<String, SignedNonceChallenge>> {
    static MAP: OnceLock<Mutex<HashMap<String, SignedNonceChallenge>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Challenge issued by a verifier peer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedNonceChallenge {
    pub challenge_id: String,
    pub nonce_hex: String,
    pub issued_at: u64,
    pub expires_at: u64,
}

/// Response proving private-key possession.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedNonceResponse {
    pub challenge_id: String,
    pub nonce_hex: String,
    pub node_id: String,
    pub public_key_hex: String,
    /// Optional ML-DSA commitment from SWAL vault (Fase 0 derive) — public only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ml_dsa_commitment_hex: Option<String>,
    pub signature_hex: String,
    pub timestamp: u64,
}

/// Canonical bytes signed for a challenge (domain-separated).
pub fn challenge_sign_payload(challenge: &SignedNonceChallenge, node_id: &str) -> Vec<u8> {
    format!(
        "swal-mesh-signed-nonce-v1|{}|{}|{}|{}|{}",
        challenge.challenge_id,
        challenge.nonce_hex,
        challenge.issued_at,
        challenge.expires_at,
        node_id
    )
    .into_bytes()
}

/// Issue a fresh random nonce challenge (stored one-shot until verify or expiry).
pub fn create_signed_nonce_challenge(ttl_secs: Option<u64>) -> SignedNonceChallenge {
    let mut nonce = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let issued_at = now_secs();
    let ttl = ttl_secs.unwrap_or(DEFAULT_TTL_SECS);
    let challenge = SignedNonceChallenge {
        challenge_id: uuid::Uuid::new_v4().to_string(),
        nonce_hex: crate::crypto::hex_encode(nonce),
        issued_at,
        expires_at: issued_at + ttl,
    };
    if let Ok(mut map) = pending().lock() {
        map.insert(challenge.challenge_id.clone(), challenge.clone());
    }
    challenge
}

/// Sign a challenge with the local mesh identity.
pub fn sign_nonce_challenge(
    identity: &NodeIdentity,
    challenge: &SignedNonceChallenge,
) -> Result<SignedNonceResponse> {
    if now_secs() > challenge.expires_at {
        bail!("challenge expired");
    }
    let payload = challenge_sign_payload(challenge, identity.node_id.as_str());
    let signature_hex = crate::crypto::hex_encode(identity.sign(&payload));
    Ok(SignedNonceResponse {
        challenge_id: challenge.challenge_id.clone(),
        nonce_hex: challenge.nonce_hex.clone(),
        node_id: identity.node_id.as_str().to_string(),
        public_key_hex: crate::crypto::hex_encode(&identity.public_key),
        ml_dsa_commitment_hex: identity.ml_dsa_commitment_hex(),
        signature_hex,
        timestamp: now_secs(),
    })
}

/// Verify a signed nonce response. Consumes the pending challenge (one-shot).
pub fn verify_nonce_response(response: &SignedNonceResponse) -> Result<NodeId> {
    let challenge = {
        let mut map = pending()
            .lock()
            .map_err(|_| anyhow::anyhow!("challenge map poisoned"))?;
        map.remove(&response.challenge_id)
            .ok_or_else(|| anyhow::anyhow!("unknown or already-used challenge"))?
    };

    if now_secs() > challenge.expires_at {
        bail!("challenge expired");
    }
    if response.nonce_hex != challenge.nonce_hex {
        bail!("nonce mismatch");
    }

    let node_id = NodeId::parse(&response.node_id)?;
    let pk = crate::crypto::hex_decode(&response.public_key_hex)?;
    let expected_id = NodeId::from_public_key_bytes(&pk);
    if expected_id != node_id {
        bail!("node_id does not match public key");
    }

    let sig = crate::crypto::hex_decode(&response.signature_hex)?;
    let payload = challenge_sign_payload(&challenge, node_id.as_str());
    if !NodeIdentity::verify(&pk, &payload, &sig) {
        bail!("invalid challenge signature");
    }

    Ok(node_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_roundtrip_ed25519() {
        let identity = NodeIdentity::generate();
        let challenge = create_signed_nonce_challenge(Some(30));
        let response = sign_nonce_challenge(&identity, &challenge).unwrap();
        let verified = verify_nonce_response(&response).unwrap();
        assert_eq!(verified, identity.node_id);
        // one-shot: replay fails
        assert!(verify_nonce_response(&response).is_err());
    }

    #[test]
    fn forged_signature_rejected() {
        let a = NodeIdentity::generate();
        let b = NodeIdentity::generate();
        let challenge = create_signed_nonce_challenge(Some(30));
        let mut response = sign_nonce_challenge(&a, &challenge).unwrap();
        response.public_key_hex = crate::crypto::hex_encode(&b.public_key);
        response.node_id = b.node_id.as_str().to_string();
        // signature still from A → must fail
        assert!(verify_nonce_response(&response).is_err());
    }
}
