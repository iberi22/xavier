//! Closed encrypted P2P network per Space (T-07)
//!
//! E2E via EncryptedSessionPayload (AES wallet session key). Transport
//! priority: Iroh QUIC -> Tor arti onion v3 -> BYO CF Relay R2/D1.
//! This module models the local registry and E2E envelope; actual
//! transport dial is wired via `mesh::iroh_transport` in follow-up.

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

/// Encrypted envelope for P2P payload (stub AES)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    pub network_id: String,
    pub sender: String,
    pub ciphertext_hex: String,
    pub nonce_hex: String,
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

    /// Encrypt a payload for the network (stub: hex-encode with sender prefix)
    pub fn encrypt(&self, network_id: &str, sender: &str, plaintext: &[u8]) -> EncryptedEnvelope {
        // Stub E2E: real impl will use AES-GCM with wallet session key derived from wallet_id
        // Use fixed 32-byte dummy key so decrypt can strip it deterministically
        let dummy_key = b"0123456789ABCDEF0123456789ABCDEF"; // 32 bytes
        let mut data = Vec::new();
        data.extend_from_slice(dummy_key);
        data.extend_from_slice(plaintext);
        // Include network_id/sender in envelope metadata for routing, not in ciphertext prefix
        let _ = (network_id, sender);
        EncryptedEnvelope {
            network_id: network_id.to_string(),
            sender: sender.to_string(),
            ciphertext_hex: crate::crypto::hex_encode(&data),
            nonce_hex: crate::crypto::hex_encode(b"nonce12bytes"),
        }
    }

    /// Decrypt a payload (stub)
    pub fn decrypt(
        &self,
        envelope: &EncryptedEnvelope,
        _network_id: &str,
    ) -> Result<Vec<u8>, String> {
        let raw = crate::crypto::hex_decode(&envelope.ciphertext_hex).map_err(|e| e.to_string())?;
        // Strip fixed 32-byte dummy key prefix
        if raw.len() < 32 {
            return Err("ciphertext too short".into());
        }
        // Return everything after 32-byte prefix as plaintext
        Ok(raw[32..].to_vec())
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

    #[test]
    fn encrypt_decrypt_stub() {
        let mgr = ClosedNetworkManager::new();
        let env = mgr.encrypt("net_123", "n1", b"hello world");
        assert_eq!(env.sender, "n1");
        assert!(!env.ciphertext_hex.is_empty());
        let plain = mgr.decrypt(&env, "net_123").unwrap();
        assert!(plain.windows(11).any(|w| w == b"hello world"));
    }
}
