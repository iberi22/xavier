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
#[derive(Debug, Default, Clone)]
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

    /// Push an encrypted envelope to a remote peer node using mesh HTTP sync transport.
    /// Gated behind `espacio-mesh` feature / runtime flag.
    #[cfg(feature = "espacio-mesh")]
    pub async fn sync_push_envelope(
        &self,
        _transport: &crate::mesh::MeshTransport,
        peer_url: &str,
        token: &str,
        envelope: &EncryptedEnvelope,
    ) -> anyhow::Result<usize> {
        let url = format!("{}/v1/mesh/espacio/sync", peer_url.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let resp = client
            .post(&url)
            .header("X-Xavier-Token", token)
            .json(envelope)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Mesh HTTP sync failed: {}", e))?;

        if !resp.status().is_success() {
            anyhow::bail!("Mesh HTTP sync rejected with status: {}", resp.status());
        }

        #[derive(serde::Deserialize)]
        struct SyncResp {
            applied: usize,
        }

        let body: SyncResp = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse sync response: {}", e))?;
        Ok(body.applied)
    }

    /// Pack and encrypt local channel messages and send them to a remote peer node.
    #[cfg(feature = "espacio-mesh")]
    pub async fn sync_channel_to_peer(
        &self,
        transport: &crate::mesh::MeshTransport,
        peer_url: &str,
        token: &str,
        channels: &crate::espacio::channel::ChannelManager,
        network_id: &str,
        sender_node: &str,
        key: &[u8; 32],
        since_seq: u64,
    ) -> anyhow::Result<usize> {
        let network = self
            .get(network_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Network not found"))?;

        if !self.is_member(network_id, sender_node).await {
            anyhow::bail!("Sender is not a member of network {}", network_id);
        }

        let messages = channels.list_since(&network.space_id, since_seq).await;
        if messages.is_empty() {
            return Ok(0);
        }

        let plaintext = serde_json::to_vec(&messages)
            .map_err(|e| anyhow::anyhow!("Failed to serialize messages: {}", e))?;
        let envelope = self
            .encrypt(network_id, sender_node, &plaintext, key)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        self.sync_push_envelope(transport, peer_url, token, &envelope)
            .await
    }
}

/// Returns the number of messages in the received batch.
pub async fn receive_channel_batch(
    net: &ClosedNetworkManager,
    channels: &crate::espacio::channel::ChannelManager,
    network_id: &str,
    receiver_node: &str,
    envelope: &EncryptedEnvelope,
    key: &[u8; 32],
) -> Result<usize, String> {
    if !net.is_member(network_id, receiver_node).await {
        return Err("receiver not a member".into());
    }
    if !net.is_member(network_id, &envelope.sender).await {
        return Err("sender not a member".into());
    }
    let network = net.get(network_id).await.ok_or("network not found")?;

    let plaintext = net.decrypt(envelope, network_id, key)?;

    let messages: Vec<crate::espacio::channel::ChannelMessage> =
        serde_json::from_slice(&plaintext).map_err(|e| e.to_string())?;

    for msg in &messages {
        if msg.space_id != network.space_id {
            return Err("foreign space message rejected".into());
        }
    }

    let count = messages.len();
    channels.merge(network.space_id.clone(), messages).await;
    Ok(count)
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

    #[tokio::test]
    async fn non_member_is_refused() {
        let mgr = ClosedNetworkManager::new();
        let net = mgr.create("esp_a".into(), vec!["n1".into()]).await;
        let channels = crate::espacio::channel::ChannelManager::new();
        let env = mgr.encrypt(&net.network_id, "n1", b"[]", &KEY).unwrap();
        let res =
            super::receive_channel_batch(&mgr, &channels, &net.network_id, "n2", &env, &KEY).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn envelope_sender_not_a_member_is_rejected() {
        let mgr = ClosedNetworkManager::new();
        let net = mgr.create("esp_a".into(), vec!["n1".into()]).await;
        let channels = crate::espacio::channel::ChannelManager::new();
        let env = mgr.encrypt(&net.network_id, "n2", b"[]", &KEY).unwrap();
        let res =
            super::receive_channel_batch(&mgr, &channels, &net.network_id, "n1", &env, &KEY).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn foreign_space_messages_are_rejected() {
        let mgr = ClosedNetworkManager::new();
        let net = mgr.create("esp_a".into(), vec!["n1".into()]).await;
        let channels = crate::espacio::channel::ChannelManager::new();
        let msg = crate::espacio::channel::ChannelMessage {
            seq: 1,
            space_id: "esp_b".into(),
            author: "n1".into(),
            content: "hello".into(),
            created_at: chrono::Utc::now(),
        };
        let pt = serde_json::to_vec(&vec![msg]).unwrap();
        let env = mgr.encrypt(&net.network_id, "n1", &pt, &KEY).unwrap();
        let res =
            super::receive_channel_batch(&mgr, &channels, &net.network_id, "n1", &env, &KEY).await;
        assert!(res.is_err());
        assert_eq!(channels.len("esp_a").await, 0);
        assert_eq!(channels.len("esp_b").await, 0);
    }

    #[tokio::test]
    async fn receive_merges_into_channel_log() {
        let mgr = ClosedNetworkManager::new();
        let net = mgr.create("esp_a".into(), vec!["n1".into()]).await;
        let channels = crate::espacio::channel::ChannelManager::new();
        let msg = crate::espacio::channel::ChannelMessage {
            seq: 1,
            space_id: "esp_a".into(),
            author: "n1".into(),
            content: "hello".into(),
            created_at: chrono::Utc::now(),
        };
        let pt = serde_json::to_vec(&vec![msg]).unwrap();
        let env = mgr.encrypt(&net.network_id, "n1", &pt, &KEY).unwrap();
        let res =
            super::receive_channel_batch(&mgr, &channels, &net.network_id, "n1", &env, &KEY).await;
        assert_eq!(res.unwrap(), 1);
        let msgs = channels.list_all("esp_a").await;
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].seq, 1);
        assert_eq!(msgs[0].content, "hello");
    }

    #[tokio::test]
    async fn second_receive_is_idempotent_by_seq() {
        let mgr = ClosedNetworkManager::new();
        let net = mgr.create("esp_a".into(), vec!["n1".into()]).await;
        let channels = crate::espacio::channel::ChannelManager::new();
        let msg = crate::espacio::channel::ChannelMessage {
            seq: 1,
            space_id: "esp_a".into(),
            author: "n1".into(),
            content: "hello".into(),
            created_at: chrono::Utc::now(),
        };
        let pt = serde_json::to_vec(&vec![msg]).unwrap();
        let env = mgr.encrypt(&net.network_id, "n1", &pt, &KEY).unwrap();
        let res1 =
            super::receive_channel_batch(&mgr, &channels, &net.network_id, "n1", &env, &KEY).await;
        assert_eq!(res1.unwrap(), 1);
        let res2 =
            super::receive_channel_batch(&mgr, &channels, &net.network_id, "n1", &env, &KEY).await;
        assert_eq!(res2.unwrap(), 1);
        assert_eq!(channels.len("esp_a").await, 1);
    }

    #[tokio::test]
    async fn wrong_key_is_rejected_on_receive() {
        let mgr = ClosedNetworkManager::new();
        let net = mgr.create("esp_a".into(), vec!["n1".into()]).await;
        let channels = crate::espacio::channel::ChannelManager::new();
        let env = mgr.encrypt(&net.network_id, "n1", b"[]", &KEY).unwrap();
        let wrong_key = [8u8; 32];
        let res =
            super::receive_channel_batch(&mgr, &channels, &net.network_id, "n1", &env, &wrong_key)
                .await;
        assert!(res.is_err());
    }

    #[cfg(feature = "espacio-mesh")]
    #[tokio::test]
    async fn test_sync_channel_to_peer_empty_messages() {
        let mgr = ClosedNetworkManager::new();
        let net = mgr.create("esp_a".into(), vec!["n1".into()]).await;
        let channels = crate::espacio::channel::ChannelManager::new();
        let identity = std::sync::Arc::new(crate::mesh::NodeIdentity::generate());
        let transport = crate::mesh::MeshTransport::new(identity);

        // With no messages in channel, sync_channel_to_peer returns Ok(0) without HTTP network call
        let count = mgr
            .sync_channel_to_peer(
                &transport,
                "http://127.0.0.1:9",
                "token",
                &channels,
                &net.network_id,
                "n1",
                &KEY,
                0,
            )
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[cfg(feature = "espacio-mesh")]
    #[tokio::test]
    async fn test_sync_channel_to_peer_non_member_rejected() {
        let mgr = ClosedNetworkManager::new();
        let net = mgr.create("esp_a".into(), vec!["n1".into()]).await;
        let channels = crate::espacio::channel::ChannelManager::new();
        let identity = std::sync::Arc::new(crate::mesh::NodeIdentity::generate());
        let transport = crate::mesh::MeshTransport::new(identity);

        let res = mgr
            .sync_channel_to_peer(
                &transport,
                "http://127.0.0.1:9",
                "token",
                &channels,
                &net.network_id,
                "n2", // not a member
                &KEY,
                0,
            )
            .await;
        assert!(res.is_err());
    }
}
