//! Closed encrypted P2P network per Space (T-07)
//!
//! E2E envelope: AES-256-GCM with a caller-supplied 32-byte key (key
//! management is not part of this module). Transport
//! priority: Iroh QUIC -> Tor arti onion v3 -> BYO CF Relay R2/D1.
//! This module models the local registry and E2E envelope; actual
//! transport dial is wired via `mesh::iroh_transport` in follow-up.

use crate::crypto::encryption::NonceBytes;
use crate::crypto::NONCE_SIZE;
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Closed network for a Space (E2E, only members)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedNetwork {
    pub space_id: String,
    pub network_id: String,
    pub members: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Encrypted envelope for P2P payload (AES-256-GCM)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    pub network_id: String,
    pub sender: String,
    /// hex of version || nonce || ciphertext+tag
    pub ciphertext_hex: String,
}

const ENVELOPE_VERSION: u8 = 1;

/// Length-prefixed associated data: version || len(net) || net || len(sender) || sender
fn associated_data(network_id: &str, sender: &str) -> Vec<u8> {
    let mut aad = vec![ENVELOPE_VERSION];
    for part in [network_id, sender] {
        aad.extend_from_slice(&(part.len() as u32).to_be_bytes());
        aad.extend_from_slice(part.as_bytes());
    }
    aad
}

/// Manager for closed networks per Space
#[derive(Debug, Default)]
pub struct ClosedNetworkManager {
    networks: Arc<RwLock<HashMap<String, ClosedNetwork>>>,
}

impl ClosedNetworkManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a closed network for a Space with initial members
    pub async fn create(&self, space_id: String, members: Vec<String>) -> ClosedNetwork {
        let network = ClosedNetwork {
            space_id: space_id.clone(),
            network_id: format!("net_{}", ulid::Ulid::new()),
            members,
            created_at: chrono::Utc::now(),
        };
        let id = network.network_id.clone();
        self.networks.write().await.insert(id, network.clone());
        network
    }

    /// Get a network by id
    pub async fn get(&self, network_id: &str) -> Option<ClosedNetwork> {
        self.networks.read().await.get(network_id).cloned()
    }

    /// Check if a node is member of the network
    pub async fn is_member(&self, network_id: &str, node_id: &str) -> bool {
        self.networks
            .read()
            .await
            .get(network_id)
            .map(|n| n.members.contains(&node_id.to_string()))
            .unwrap_or(false)
    }

    /// Add a member (admin only, checked externally via `can`)
    pub async fn add_member(&self, network_id: &str, node_id: String) -> Result<(), String> {
        let mut guard = self.networks.write().await;
        let net = guard.get_mut(network_id).ok_or("network not found")?;
        if net.members.contains(&node_id) {
            return Err("already member".into());
        }
        net.members.push(node_id);
        Ok(())
    }

    /// Encrypt a payload with AES-256-GCM under a caller-supplied key.
    /// Envelope layout (before hex): version(1) || nonce(12) || ciphertext+tag.
    /// Associated data binds the envelope version, network id and sender.
    pub fn encrypt(
        &self,
        network_id: &str,
        sender: &str,
        plaintext: &[u8],
        key: &[u8; 32],
    ) -> Result<EncryptedEnvelope, String> {
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "invalid key".to_string())?;
        let nonce = NonceBytes::generate();
        let aad = associated_data(network_id, sender);
        let ct = cipher
            .encrypt(
                Nonce::from_slice(nonce.as_bytes()),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| "encryption failed".to_string())?;
        let mut data = Vec::with_capacity(1 + NONCE_SIZE + ct.len());
        data.push(ENVELOPE_VERSION);
        data.extend_from_slice(nonce.as_bytes());
        data.extend_from_slice(&ct);
        Ok(EncryptedEnvelope {
            network_id: network_id.to_string(),
            sender: sender.to_string(),
            ciphertext_hex: crate::crypto::hex_encode(&data),
        })
    }

    /// Decrypt an envelope. Fails closed on tamper, wrong key, or wrong
    /// network/sender (associated data mismatch).
    pub fn decrypt(
        &self,
        envelope: &EncryptedEnvelope,
        network_id: &str,
        key: &[u8; 32],
    ) -> Result<Vec<u8>, String> {
        if envelope.network_id != network_id {
            return Err("network id mismatch".into());
        }
        let raw = crate::crypto::hex_decode(&envelope.ciphertext_hex).map_err(|e| e.to_string())?;
        if raw.len() < 1 + NONCE_SIZE + 16 {
            return Err("ciphertext too short".into());
        }
        if raw[0] != ENVELOPE_VERSION {
            return Err("unsupported envelope version".into());
        }
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "invalid key".to_string())?;
        let aad = associated_data(network_id, &envelope.sender);
        cipher
            .decrypt(
                Nonce::from_slice(&raw[1..1 + NONCE_SIZE]),
                Payload {
                    msg: &raw[1 + NONCE_SIZE..],
                    aad: &aad,
                },
            )
            .map_err(|_| "authentication failed".to_string())
    }

    /// List networks for a Space
    pub async fn list_for_space(&self, space_id: &str) -> Vec<ClosedNetwork> {
        self.networks
            .read()
            .await
            .values()
            .filter(|n| n.space_id == space_id)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_membership() {
        let mgr = ClosedNetworkManager::new();
        let net = mgr
            .create("esp_a".into(), vec!["n1".into(), "n2".into()])
            .await;
        assert!(net.network_id.starts_with("net_"));
        assert!(mgr.is_member(&net.network_id, "n1").await);
        assert!(!mgr.is_member(&net.network_id, "n3").await);
        assert_eq!(mgr.list_for_space("esp_a").await.len(), 1);
    }

    #[tokio::test]
    async fn add_member() {
        let mgr = ClosedNetworkManager::new();
        let net = mgr.create("esp_a".into(), vec!["n1".into()]).await;
        mgr.add_member(&net.network_id, "n2".into()).await.unwrap();
        assert!(mgr.is_member(&net.network_id, "n2").await);
        assert!(mgr.add_member(&net.network_id, "n2".into()).await.is_err());
    }

    const KEY: [u8; 32] = [7u8; 32];

    fn enc(mgr: &ClosedNetworkManager, pt: &[u8]) -> EncryptedEnvelope {
        mgr.encrypt("net_123", "n1", pt, &KEY).unwrap()
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let mgr = ClosedNetworkManager::new();
        let env = enc(&mgr, b"hello world");
        assert_eq!(env.sender, "n1");
        assert!(!env
            .ciphertext_hex
            .contains(&crate::crypto::hex_encode(b"hello world")));
        assert_eq!(mgr.decrypt(&env, "net_123", &KEY).unwrap(), b"hello world");
    }

    #[test]
    fn tamper_is_rejected() {
        let mgr = ClosedNetworkManager::new();
        let env = enc(&mgr, b"hello world");
        let mut raw = crate::crypto::hex_decode(&env.ciphertext_hex).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        let bad = EncryptedEnvelope {
            ciphertext_hex: crate::crypto::hex_encode(&raw),
            ..env.clone()
        };
        assert!(mgr.decrypt(&bad, "net_123", &KEY).is_err());
        let mut raw = crate::crypto::hex_decode(&env.ciphertext_hex).unwrap();
        raw[2] ^= 1; // nonce byte
        let bad = EncryptedEnvelope {
            ciphertext_hex: crate::crypto::hex_encode(&raw),
            ..env
        };
        assert!(mgr.decrypt(&bad, "net_123", &KEY).is_err());
    }

    #[test]
    fn wrong_key_is_rejected() {
        let mgr = ClosedNetworkManager::new();
        let env = enc(&mgr, b"secret");
        assert!(mgr.decrypt(&env, "net_123", &[8u8; 32]).is_err());
    }

    #[test]
    fn nonces_are_unique() {
        let mgr = ClosedNetworkManager::new();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..500 {
            let env = enc(&mgr, b"same");
            let raw = crate::crypto::hex_decode(&env.ciphertext_hex).unwrap();
            assert!(seen.insert(raw[1..1 + NONCE_SIZE].to_vec()));
        }
    }

    #[test]
    fn associated_data_mismatch_is_rejected() {
        let mgr = ClosedNetworkManager::new();
        let env = enc(&mgr, b"bound");
        let other_sender = EncryptedEnvelope {
            sender: "n2".into(),
            ..env.clone()
        };
        assert!(mgr.decrypt(&other_sender, "net_123", &KEY).is_err());
        let other_net = EncryptedEnvelope {
            network_id: "net_999".into(),
            ..env.clone()
        };
        assert!(mgr.decrypt(&other_net, "net_999", &KEY).is_err());
        assert!(mgr.decrypt(&env, "net_999", &KEY).is_err());
    }

    #[test]
    fn short_or_bad_version_is_rejected() {
        let mgr = ClosedNetworkManager::new();
        let env = enc(&mgr, b"x");
        let mut raw = crate::crypto::hex_decode(&env.ciphertext_hex).unwrap();
        raw[0] = 9;
        let bad = EncryptedEnvelope {
            ciphertext_hex: crate::crypto::hex_encode(&raw),
            ..env.clone()
        };
        assert!(mgr.decrypt(&bad, "net_123", &KEY).is_err());
        let short = EncryptedEnvelope {
            ciphertext_hex: "0102".into(),
            ..env
        };
        assert!(mgr.decrypt(&short, "net_123", &KEY).is_err());
    }
}
