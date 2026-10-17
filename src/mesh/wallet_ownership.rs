//! Wallet ownership proof for private-mesh node registration.
//!
//! A wallet id is the SHA-256 of its owner's Ed25519 public key. Registering a
//! node under a wallet requires a one-shot challenge issued by the server and
//! signed with that key. The signed payload binds the challenge, the wallet id
//! and the node id, so a signature cannot be replayed for another wallet, another
//! node, or after the challenge has been used or has expired.

use anyhow::{anyhow, bail, Result};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::mesh::node::NodeIdentity;
use crate::mesh::private_mesh::derive_wallet_id;

const CHALLENGE_TTL_SECS: u64 = 60;
const MAX_PENDING_CHALLENGES: usize = 1024;
const SIGN_DOMAIN: &str = "swal-private-mesh-wallet-register-v1";

/// Challenge issued for a single wallet registration attempt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalletChallenge {
    pub challenge_id: String,
    pub wallet_id: String,
    pub nonce_hex: String,
    pub expires_at: u64,
}

/// Proof that the caller holds the Ed25519 key behind a wallet id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalletRegistrationProof {
    pub challenge_id: String,
    pub public_key_hex: String,
    pub signature_hex: String,
}

/// Pending one-shot challenges keyed by challenge_id.
fn pending() -> &'static Mutex<HashMap<String, WalletChallenge>> {
    static MAP: OnceLock<Mutex<HashMap<String, WalletChallenge>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Issue a fresh challenge for `wallet_id`. Expired challenges are pruned first,
/// and the pending set is capped so unused challenges cannot grow without bound.
pub fn issue_wallet_challenge(wallet_id: &str) -> Result<WalletChallenge> {
    let now = now_secs();
    let mut map = pending()
        .lock()
        .map_err(|_| anyhow!("challenge map poisoned"))?;
    map.retain(|_, challenge| challenge.expires_at >= now);
    if map.len() >= MAX_PENDING_CHALLENGES {
        bail!("too many pending wallet challenges");
    }

    let mut nonce = [0u8; 32];
    OsRng.fill_bytes(&mut nonce);
    let challenge = WalletChallenge {
        challenge_id: uuid::Uuid::new_v4().to_string(),
        wallet_id: wallet_id.to_string(),
        nonce_hex: crate::crypto::hex_encode(nonce),
        expires_at: now + CHALLENGE_TTL_SECS,
    };
    map.insert(challenge.challenge_id.clone(), challenge.clone());
    Ok(challenge)
}

/// Canonical bytes signed for a registration: domain, challenge, wallet and node.
pub fn wallet_registration_payload(challenge: &WalletChallenge, node_id: &str) -> Vec<u8> {
    format!(
        "{}|{}|{}|{}|{}|{}",
        SIGN_DOMAIN,
        challenge.challenge_id,
        challenge.nonce_hex,
        challenge.wallet_id,
        node_id,
        challenge.expires_at
    )
    .into_bytes()
}

/// Sign a challenge with the wallet's Ed25519 identity for `node_id`.
pub fn sign_wallet_registration(
    wallet_key: &NodeIdentity,
    challenge: &WalletChallenge,
    node_id: &str,
) -> WalletRegistrationProof {
    let payload = wallet_registration_payload(challenge, node_id);
    WalletRegistrationProof {
        challenge_id: challenge.challenge_id.clone(),
        public_key_hex: crate::crypto::hex_encode(&wallet_key.public_key),
        signature_hex: crate::crypto::hex_encode(wallet_key.sign(&payload)),
    }
}

/// Verify a registration proof for `wallet_id` and `node_id`.
///
/// The challenge is consumed before any check, so each challenge gets exactly one
/// attempt whether or not the proof is valid.
pub fn verify_wallet_registration(
    proof: &WalletRegistrationProof,
    wallet_id: &str,
    node_id: &str,
) -> Result<()> {
    let challenge = {
        let mut map = pending()
            .lock()
            .map_err(|_| anyhow!("challenge map poisoned"))?;
        map.remove(&proof.challenge_id)
            .ok_or_else(|| anyhow!("unknown or already-used challenge"))?
    };

    if now_secs() > challenge.expires_at {
        bail!("challenge expired");
    }
    if challenge.wallet_id != wallet_id {
        bail!("challenge was issued for a different wallet");
    }

    let public_key = crate::crypto::hex_decode(&proof.public_key_hex)?;
    if derive_wallet_id(&public_key) != wallet_id {
        bail!("public key does not match wallet id");
    }

    let signature = crate::crypto::hex_decode(&proof.signature_hex)?;
    let payload = wallet_registration_payload(&challenge, node_id);
    if !NodeIdentity::verify(&public_key, &payload, &signature) {
        bail!("invalid registration signature");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wallet_of(key: &NodeIdentity) -> String {
        derive_wallet_id(&key.public_key)
    }

    #[test]
    fn registration_proof_roundtrip_and_replay_rejected() {
        let key = NodeIdentity::generate();
        let wallet = wallet_of(&key);
        let challenge = issue_wallet_challenge(&wallet).unwrap();
        let proof = sign_wallet_registration(&key, &challenge, "node-1");

        verify_wallet_registration(&proof, &wallet, "node-1").unwrap();
        // one-shot: the same proof cannot register again
        assert!(verify_wallet_registration(&proof, &wallet, "node-1").is_err());
    }

    #[test]
    fn claimed_wallet_without_its_key_rejected() {
        let owner = NodeIdentity::generate();
        let attacker = NodeIdentity::generate();
        let wallet = wallet_of(&owner);
        let challenge = issue_wallet_challenge(&wallet).unwrap();
        // attacker signs with their own key but claims the owner's wallet
        let proof = sign_wallet_registration(&attacker, &challenge, "node-x");

        assert!(verify_wallet_registration(&proof, &wallet, "node-x").is_err());
    }

    #[test]
    fn proof_bound_to_node_id() {
        let key = NodeIdentity::generate();
        let wallet = wallet_of(&key);
        let challenge = issue_wallet_challenge(&wallet).unwrap();
        let proof = sign_wallet_registration(&key, &challenge, "node-a");

        assert!(verify_wallet_registration(&proof, &wallet, "node-b").is_err());
    }

    #[test]
    fn challenge_bound_to_wallet_id() {
        let key = NodeIdentity::generate();
        let wallet = wallet_of(&key);
        let other = NodeIdentity::generate();
        let other_wallet = wallet_of(&other);
        let challenge = issue_wallet_challenge(&wallet).unwrap();
        let proof = sign_wallet_registration(&key, &challenge, "node-1");

        assert!(verify_wallet_registration(&proof, &other_wallet, "node-1").is_err());
    }

    #[test]
    fn expired_challenge_rejected() {
        let key = NodeIdentity::generate();
        let wallet = wallet_of(&key);
        let mut challenge = issue_wallet_challenge(&wallet).unwrap();
        challenge.expires_at = now_secs() - 1;
        pending()
            .lock()
            .unwrap()
            .insert(challenge.challenge_id.clone(), challenge.clone());
        let proof = sign_wallet_registration(&key, &challenge, "node-1");

        assert!(verify_wallet_registration(&proof, &wallet, "node-1").is_err());
    }

    #[test]
    fn unknown_challenge_rejected() {
        let key = NodeIdentity::generate();
        let wallet = wallet_of(&key);
        let challenge = WalletChallenge {
            challenge_id: uuid::Uuid::new_v4().to_string(),
            wallet_id: wallet.clone(),
            nonce_hex: "00".repeat(32),
            expires_at: now_secs() + 60,
        };
        let proof = sign_wallet_registration(&key, &challenge, "node-1");

        assert!(verify_wallet_registration(&proof, &wallet, "node-1").is_err());
    }
}
