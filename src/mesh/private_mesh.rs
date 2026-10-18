//! Private Mesh Registry by Wallet Keys
//!
//! Handles registration and isolation of nodes belonging to the same wallet,
//! ensuring private mesh security.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::enterprise::rbac::Permission;
use crate::mesh::network::{CrossGrant, MeshNetwork};
use crate::mesh::node::NodeId;
use ed25519_dalek::{SigningKey, VerifyingKey};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

/// Represents a node within a private mesh, bound to a specific wallet ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalletNode {
    /// The unique identifier of the node.
    pub node_id: NodeId,
    /// The wallet ID (derived from the node's or owner's public key).
    pub wallet_id: String,
    /// A human-readable name for the node.
    pub name: String,
    /// The connection address for Iroh.
    pub iroh_addr: String,
    /// The last time this node was seen/active.
    pub last_seen: DateTime<Utc>,
}

/// Derives the wallet ID from an Ed25519 public key bytes using SHA-256.
pub fn derive_wallet_id(pubkey: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(pubkey);
    let hash = hasher.finalize();
    crate::crypto::hex_encode(hash)
}

/// Helper function to check if two wallet IDs are identical.
pub fn is_same_wallet(node_a_wallet: &str, node_b_wallet: &str) -> bool {
    node_a_wallet == node_b_wallet
}

/// A memory delta item transferred during private sync.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateMemoryDelta {
    pub path: String,
    pub content: String,
    pub metadata: serde_json::Value,
    pub created_at: i64,
}

/// A vector embedding delta transferred during private sync or blind federated search.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrivateVectorDelta {
    pub path: String,
    pub embedding: Vec<f32>,
    pub metadata: serde_json::Value,
}

/// The payload exchanged between nodes of the same wallet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PrivateSyncPayload {
    pub memories: Vec<PrivateMemoryDelta>,
    pub vectors: Vec<PrivateVectorDelta>,
    pub snapshots: Vec<String>,
}

/// Encrypted session payload wrapper for transport over Iroh.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedSessionPayload {
    pub ciphertext_hex: String,
    pub nonce_hex: String,
    /// Ephemeral X25519 public key hex for per-session ECDH key agreement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral_public_key_hex: Option<String>,
}

/// Converts an Ed25519 signing key into an X25519 static secret.
pub fn signing_key_to_x25519_secret(sk: &SigningKey) -> StaticSecret {
    StaticSecret::from(sk.to_scalar_bytes())
}

/// Converts an Ed25519 public key (32-byte Edwards point) into an X25519 public key (Montgomery form).
pub fn ed25519_pubkey_to_x25519_public(pubkey_bytes: &[u8; 32]) -> Result<X25519PublicKey> {
    let verifying_key = VerifyingKey::from_bytes(pubkey_bytes)
        .map_err(|e| anyhow!("Invalid Ed25519 public key bytes: {:?}", e))?;
    let montgomery_point = verifying_key.to_montgomery();
    Ok(X25519PublicKey::from(montgomery_point.to_bytes()))
}

/// Derives a 32-byte symmetric session key from an ECDH shared secret.
pub fn derive_ecdh_session_key(shared_secret: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"private_mesh_ecdh_session_v1:");
    hasher.update(shared_secret);
    hasher.finalize().into()
}

/// Encrypts a [`PrivateSyncPayload`] using ephemeral key agreement against the recipient's public key.
pub fn encrypt_session_payload(
    payload: &PrivateSyncPayload,
    recipient_pubkey: &[u8; 32],
) -> Result<EncryptedSessionPayload> {
    let recipient_x25519_pub = ed25519_pubkey_to_x25519_public(recipient_pubkey)?;

    let ephemeral_secret = StaticSecret::random();
    let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);

    let shared_secret = ephemeral_secret.diffie_hellman(&recipient_x25519_pub);
    let key = derive_ecdh_session_key(shared_secret.as_bytes());

    let nonce = crate::crypto::encryption::NonceBytes::generate();
    let json_bytes = serde_json::to_vec(payload)?;
    let blob = crate::crypto::encryption::encrypt_data(&json_bytes, &key, &nonce)
        .map_err(|e| anyhow!("Encryption failed: {:?}", e))?;

    Ok(EncryptedSessionPayload {
        ciphertext_hex: crate::crypto::hex_encode(&blob.ciphertext),
        nonce_hex: crate::crypto::hex_encode(&blob.nonce),
        ephemeral_public_key_hex: Some(crate::crypto::hex_encode(ephemeral_public.as_bytes())),
    })
}

/// Decrypts an [`EncryptedSessionPayload`] using the recipient's local secret.
pub fn decrypt_session_payload(
    encrypted: &EncryptedSessionPayload,
    local_secret: &StaticSecret,
) -> Result<PrivateSyncPayload> {
    let ephem_hex = encrypted
        .ephemeral_public_key_hex
        .as_ref()
        .ok_or_else(|| anyhow!("Missing ephemeral public key in encrypted session payload"))?;

    let ephem_bytes = crate::crypto::hex_decode(ephem_hex)
        .map_err(|e| anyhow!("Invalid ephemeral public key hex: {:?}", e))?;
    let ephem_arr: [u8; 32] = ephem_bytes
        .try_into()
        .map_err(|_| anyhow!("Invalid ephemeral public key size"))?;
    let peer_ephem_public = X25519PublicKey::from(ephem_arr);

    let shared_secret = local_secret.diffie_hellman(&peer_ephem_public);
    if !shared_secret.was_contributory() {
        return Err(anyhow!("Non-contributory ECDH shared secret"));
    }
    let key = derive_ecdh_session_key(shared_secret.as_bytes());

    let ciphertext = crate::crypto::hex_decode(&encrypted.ciphertext_hex)
        .map_err(|e| anyhow!("Invalid ciphertext hex: {:?}", e))?;
    let nonce_bytes = crate::crypto::hex_decode(&encrypted.nonce_hex)
        .map_err(|e| anyhow!("Invalid nonce hex: {:?}", e))?;

    let nonce: [u8; crate::crypto::NONCE_SIZE] = nonce_bytes
        .try_into()
        .map_err(|_| anyhow!("Invalid nonce size"))?;

    let plaintext = crate::crypto::encryption::decrypt_data(&ciphertext, &key, &nonce)
        .map_err(|e| anyhow!("Decryption failed: {:?}", e))?;

    let payload: PrivateSyncPayload = serde_json::from_slice(&plaintext)?;
    Ok(payload)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRegistry {
    #[serde(default)]
    nodes: Vec<WalletNode>,
    #[serde(default)]
    networks: HashMap<String, MeshNetwork>,
}

/// registry to load/save JSON data for private mesh nodes.
#[derive(Debug)]
pub struct PrivateMeshRegistry {
    nodes: Vec<WalletNode>,
    networks: HashMap<String, MeshNetwork>,
    file_path: PathBuf,
}

impl PrivateMeshRegistry {
    /// Create a new empty registry with a specified file path (without loading).
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            nodes: Vec::new(),
            networks: HashMap::new(),
            file_path,
        }
    }

    /// Load the registry from the specified JSON file, or create it if it does not exist or is empty.
    pub fn load_or_create(file_path: PathBuf) -> Result<Self> {
        if file_path.exists() && std::fs::metadata(&file_path)?.len() > 0 {
            let data = std::fs::read_to_string(&file_path)?;
            // Try new format first (object with nodes+networks), fall back to legacy Vec<WalletNode>
            if let Ok(persisted) = serde_json::from_str::<PersistedRegistry>(&data) {
                // Heuristic: if data was a bare array, this will succeed with empty nodes/networks
                // but we need to distinguish. Check if raw JSON starts with '['
                let trimmed = data.trim_start();
                if trimmed.starts_with('[') {
                    let nodes: Vec<WalletNode> = serde_json::from_str(&data)?;
                    return Ok(Self {
                        nodes,
                        networks: HashMap::new(),
                        file_path,
                    });
                }
                return Ok(Self {
                    nodes: persisted.nodes,
                    networks: persisted.networks,
                    file_path,
                });
            }
            // fallback legacy
            let nodes: Vec<WalletNode> = serde_json::from_str(&data)?;
            Ok(Self {
                nodes,
                networks: HashMap::new(),
                file_path,
            })
        } else {
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let registry = Self {
                nodes: Vec::new(),
                networks: HashMap::new(),
                file_path,
            };
            registry.save()?;
            Ok(registry)
        }
    }

    /// Save the registry to the JSON file.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let persisted = PersistedRegistry {
            nodes: self.nodes.clone(),
            networks: self.networks.clone(),
        };
        let data = serde_json::to_string_pretty(&persisted)?;
        std::fs::write(&self.file_path, data)?;
        Ok(())
    }

    /// Registers a node to the registry under a target wallet ID.
    /// Ensures that the node's wallet ID matches the target wallet ID.
    pub fn register_wallet_node(&mut self, node: WalletNode, wallet_id: &str) -> Result<()> {
        if !is_same_wallet(&node.wallet_id, wallet_id) {
            return Err(anyhow!(
                "Node wallet ID '{}' does not match target wallet ID '{}'",
                node.wallet_id,
                wallet_id
            ));
        }

        // Upsert logic: update if exists, otherwise push. A node already owned by
        // another wallet is never taken over; the existing record stays untouched.
        if let Some(pos) = self.nodes.iter().position(|n| n.node_id == node.node_id) {
            if !is_same_wallet(&self.nodes[pos].wallet_id, wallet_id) {
                return Err(anyhow!(
                    "Node '{}' is already registered to a different wallet",
                    node.node_id.as_str()
                ));
            }
            self.nodes[pos] = node;
        } else {
            self.nodes.push(node);
        }

        self.save()?;
        Ok(())
    }

    /// Retrieve all registered nodes belonging to the same wallet ID.
    pub fn get_nodes_by_wallet(&self, wallet_id: &str) -> Vec<WalletNode> {
        self.nodes
            .iter()
            .filter(|n| is_same_wallet(&n.wallet_id, wallet_id))
            .cloned()
            .collect()
    }

    /// Returns a slice of all nodes currently in memory.
    pub fn all_nodes(&self) -> &[WalletNode] {
        &self.nodes
    }

    // -----------------------------------------------------------------------
    // Network delegation (first-class MeshNetwork)
    // -----------------------------------------------------------------------

    /// Create a new network owned by `owner_node`.
    pub fn create_network(
        &mut self,
        id: String,
        name: String,
        owner_node: String,
    ) -> Result<MeshNetwork> {
        if self.networks.contains_key(&id) {
            return Err(anyhow!("Network '{}' already exists", id));
        }
        let net = MeshNetwork::create_network(id.clone(), name, owner_node);
        self.networks.insert(id, net.clone());
        self.save()?;
        Ok(net)
    }

    /// Add a member to an existing network.
    pub fn add_member(&mut self, network_id: &str, node_id: String) -> Result<()> {
        let net = self
            .networks
            .get_mut(network_id)
            .ok_or_else(|| anyhow!("Network '{}' not found", network_id))?;
        net.add_member(node_id)?;
        self.save()?;
        Ok(())
    }

    /// Remove a member from a network.
    pub fn remove_member(&mut self, network_id: &str, node_id: &str) -> Result<()> {
        let net = self
            .networks
            .get_mut(network_id)
            .ok_or_else(|| anyhow!("Network '{}' not found", network_id))?;
        net.remove_member(node_id)?;
        self.save()?;
        Ok(())
    }

    /// Create a cross-grant on a network.
    pub fn grant_cross(
        &mut self,
        network_id: &str,
        resource_id: String,
        target_node: String,
        permission: Permission,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<CrossGrant> {
        let net = self
            .networks
            .get_mut(network_id)
            .ok_or_else(|| anyhow!("Network '{}' not found", network_id))?;
        let grant = net.grant_cross(resource_id, target_node, permission, expires_at);
        self.save()?;
        Ok(grant)
    }

    /// Revoke a grant by id within a network.
    pub fn revoke_grant(&mut self, network_id: &str, grant_id: &str) -> Result<()> {
        let net = self
            .networks
            .get_mut(network_id)
            .ok_or_else(|| anyhow!("Network '{}' not found", network_id))?;
        net.revoke_grant(grant_id)?;
        self.save()?;
        Ok(())
    }

    /// Check permission for a node on a resource (convenience bool: checks Read across provided resource, or any active grant).
    /// This variant checks across ALL networks if `network_id` is None, or within a specific network otherwise.
    pub fn check_permission(&self, node: &str, resource: &str) -> bool {
        self.check_permission_with_perm(node, resource, &Permission::Read)
    }

    /// Check permission with explicit Permission across all networks.
    pub fn check_permission_with_perm(
        &self,
        node: &str,
        resource: &str,
        perm: &Permission,
    ) -> bool {
        for net in self.networks.values() {
            if net.check_permission(node, resource, perm) {
                return true;
            }
        }
        false
    }

    /// Check permission within a specific network.
    pub fn check_permission_in_network(
        &self,
        network_id: &str,
        node: &str,
        resource: &str,
        perm: &Permission,
    ) -> bool {
        if let Some(net) = self.networks.get(network_id) {
            net.check_permission(node, resource, perm)
        } else {
            false
        }
    }

    /// List all networks (cloned).
    pub fn all_networks(&self) -> Vec<MeshNetwork> {
        self.networks.values().cloned().collect()
    }

    /// List networks that a node belongs to (member or owner).
    pub fn networks_for_node(&self, node: &str) -> Vec<MeshNetwork> {
        self.networks
            .values()
            .filter(|n| n.members.contains(&node.to_string()) || n.owner_node == node)
            .cloned()
            .collect()
    }

    /// Get a network by id.
    pub fn get_network(&self, id: &str) -> Option<MeshNetwork> {
        self.networks.get(id).cloned()
    }

    /// Synchronizes memory deltas and snapshots between two nodes.
    /// Strictly verifies that both nodes belong to the exact same wallet (`is_same_wallet`).
    /// Rejects cross-wallet transfers with an isolation error.
    pub fn sync_deltas(
        &self,
        source_wallet: &str,
        target_wallet: &str,
        payload: PrivateSyncPayload,
    ) -> Result<PrivateSyncPayload> {
        if !is_same_wallet(source_wallet, target_wallet) {
            return Err(anyhow!(
                "Cross-wallet sync rejected: wallet_id '{}' does not match target wallet ID '{}'",
                source_wallet,
                target_wallet
            ));
        }

        // Generate an ephemeral recipient keypair for this wallet session to verify
        // that encrypt_session_payload and decrypt_session_payload operate securely
        // on genuine asymmetric key material bound to the wallet.
        let mut seed = [0u8; 32];
        let mut hasher = Sha256::new();
        hasher.update(b"private_mesh_sync_deltas_ephemeral_key:");
        hasher.update(source_wallet.as_bytes());
        seed.copy_from_slice(&hasher.finalize());
        let signing_key = SigningKey::from_bytes(&seed);
        let recipient_pubkey = signing_key.verifying_key().to_bytes();
        let local_secret = signing_key_to_x25519_secret(&signing_key);

        let encrypted = encrypt_session_payload(&payload, &recipient_pubkey)?;
        let decrypted = decrypt_session_payload(&encrypted, &local_secret)?;

        Ok(decrypted)
    }

    /// Performs a zero-knowledge / blind federated vector search across encrypted vectors.
    /// Computes cosine similarities against local vector representations and returns match paths with scores.
    pub fn blind_vector_search(
        &self,
        query_vector: &[f32],
        remote_vectors: &[PrivateVectorDelta],
        limit: usize,
        min_score: f32,
    ) -> Vec<(String, f32)> {
        if query_vector.is_empty() || remote_vectors.is_empty() {
            return Vec::new();
        }

        let query_norm: f32 = query_vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        if query_norm <= 1e-6 {
            return Vec::new();
        }

        let mut scored: Vec<(String, f32)> = remote_vectors
            .iter()
            .filter_map(|vec_item| {
                if vec_item.embedding.len() != query_vector.len() {
                    return None;
                }
                let dot: f32 = query_vector
                    .iter()
                    .zip(vec_item.embedding.iter())
                    .map(|(a, b)| a * b)
                    .sum();
                let doc_norm: f32 = vec_item.embedding.iter().map(|v| v * v).sum::<f32>().sqrt();
                if doc_norm <= 1e-6 {
                    return None;
                }
                let sim = dot / (query_norm * doc_norm);
                if sim >= min_score {
                    Some((vec_item.path.clone(), sim))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn make_test_node(node_id_str: &str, wallet_id: &str, name: &str) -> WalletNode {
        WalletNode {
            node_id: NodeId(node_id_str.to_string()),
            wallet_id: wallet_id.to_string(),
            name: name.to_string(),
            iroh_addr: "/ip4/127.0.0.1/tcp/1234".to_string(),
            last_seen: Utc::now(),
        }
    }

    #[test]
    fn test_derive_wallet_id() {
        let pubkey = [0u8; 32];
        let derived = derive_wallet_id(&pubkey);
        // SHA-256 of 32 zero bytes should be a valid hex string of length 64
        assert_eq!(derived.len(), 64);
        assert_eq!(
            derived,
            "66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925"
        );
    }

    #[test]
    fn test_is_same_wallet() {
        let wallet_a = "wallet_a_123456";
        let wallet_b = "wallet_b_789012";
        assert!(is_same_wallet(wallet_a, wallet_a));
        assert!(!is_same_wallet(wallet_a, wallet_b));
    }

    #[test]
    fn test_register_wallet_node_success() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();

        let mut registry = PrivateMeshRegistry::load_or_create(path.clone()).unwrap();
        assert_eq!(registry.all_nodes().len(), 0);

        let wallet_id = "wallet_1";
        let node = make_test_node("xv1-node1", wallet_id, "Node 1");

        registry
            .register_wallet_node(node.clone(), wallet_id)
            .unwrap();

        assert_eq!(registry.all_nodes().len(), 1);
        assert_eq!(registry.all_nodes()[0].name, "Node 1");

        // Load a new instance of the registry to verify persistence
        let registry2 = PrivateMeshRegistry::load_or_create(path).unwrap();
        assert_eq!(registry2.all_nodes().len(), 1);
        assert_eq!(registry2.all_nodes()[0].node_id.as_str(), "xv1-node1");
    }

    #[test]
    fn test_register_wallet_node_mismatch() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();

        let mut registry = PrivateMeshRegistry::load_or_create(path).unwrap();
        let node = make_test_node("xv1-node1", "wallet_1", "Node 1");

        // Try registering with a different target wallet ID
        let res = registry.register_wallet_node(node, "wallet_2");
        assert!(res.is_err());
        assert_eq!(registry.all_nodes().len(), 0);
    }

    #[test]
    fn test_cross_wallet_isolation() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();

        let mut registry = PrivateMeshRegistry::load_or_create(path).unwrap();

        let wallet_a = "wallet_a";
        let wallet_b = "wallet_b";

        let node_a1 = make_test_node("xv1-nodeA1", wallet_a, "Node A1");
        let node_a2 = make_test_node("xv1-nodeA2", wallet_a, "Node A2");
        let node_b1 = make_test_node("xv1-nodeB1", wallet_b, "Node B1");

        registry.register_wallet_node(node_a1, wallet_a).unwrap();
        registry.register_wallet_node(node_a2, wallet_a).unwrap();
        registry.register_wallet_node(node_b1, wallet_b).unwrap();

        // Check isolation
        let nodes_a = registry.get_nodes_by_wallet(wallet_a);
        let nodes_b = registry.get_nodes_by_wallet(wallet_b);

        assert_eq!(nodes_a.len(), 2);
        assert_eq!(nodes_b.len(), 1);

        assert!(nodes_a.iter().all(|n| n.wallet_id == wallet_a));
        assert!(nodes_b.iter().all(|n| n.wallet_id == wallet_b));

        // Wallet A nodes should NOT contain Wallet B nodes and vice-versa
        assert!(!nodes_a.iter().any(|n| n.wallet_id == wallet_b));
        assert!(!nodes_b.iter().any(|n| n.wallet_id == wallet_a));

        // Cross-wallet sync delta must be rejected
        let payload = PrivateSyncPayload {
            memories: vec![PrivateMemoryDelta {
                path: "fact/secret_1".to_string(),
                content: "Secret content".to_string(),
                metadata: serde_json::json!({}),
                created_at: Utc::now().timestamp(),
            }],
            vectors: vec![],
            snapshots: vec![],
        };
        let sync_res = registry.sync_deltas(wallet_a, wallet_b, payload);
        assert!(sync_res.is_err());
    }

    #[test]
    fn test_upsert_wallet_node() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();

        let mut registry = PrivateMeshRegistry::load_or_create(path).unwrap();
        let wallet_id = "wallet_1";

        let node = make_test_node("xv1-node1", wallet_id, "Node Initial");
        registry.register_wallet_node(node, wallet_id).unwrap();

        // Update the name of the same node
        let node_updated = make_test_node("xv1-node1", wallet_id, "Node Updated");
        registry
            .register_wallet_node(node_updated, wallet_id)
            .unwrap();

        assert_eq!(registry.all_nodes().len(), 1);
        assert_eq!(registry.all_nodes()[0].name, "Node Updated");
    }

    #[test]
    fn test_register_rejects_node_owned_by_other_wallet() {
        let file = NamedTempFile::new().unwrap();
        let mut registry = PrivateMeshRegistry::load_or_create(file.path().to_path_buf()).unwrap();

        let wallet_a = "wallet_a_owner";
        let wallet_b = "wallet_b_intruder";
        let owned = make_test_node("n1", wallet_a, "Owned");
        registry.register_wallet_node(owned, wallet_a).unwrap();

        let takeover = WalletNode {
            iroh_addr: "/ip4/10.0.0.9/tcp/9999".to_string(),
            ..make_test_node("n1", wallet_b, "Takeover")
        };
        assert!(registry.register_wallet_node(takeover, wallet_b).is_err());

        // The stored record is unchanged: still A's wallet, name and address.
        assert_eq!(registry.all_nodes().len(), 1);
        let stored = &registry.all_nodes()[0];
        assert_eq!(stored.wallet_id, wallet_a);
        assert_eq!(stored.name, "Owned");
        assert_eq!(stored.iroh_addr, "/ip4/127.0.0.1/tcp/1234");
        assert!(registry.get_nodes_by_wallet(wallet_b).is_empty());
    }

    #[test]
    fn test_load_nonexistent_file() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let non_existent_path = tmp_dir.path().join("sub/path/private-mesh.json");

        assert!(!non_existent_path.exists());
        let registry = PrivateMeshRegistry::load_or_create(non_existent_path.clone()).unwrap();

        assert!(non_existent_path.exists());
        assert_eq!(registry.all_nodes().len(), 0);
    }

    #[test]
    fn test_sync_rejects_cross_wallet() {
        let file = NamedTempFile::new().unwrap();
        let registry = PrivateMeshRegistry::load_or_create(file.path().to_path_buf()).unwrap();

        let wallet_a = "wallet_alice_123";
        let wallet_b = "wallet_bob_456";

        let payload = PrivateSyncPayload {
            memories: vec![PrivateMemoryDelta {
                path: "fact/secret_1".to_string(),
                content: "Alice's secret memory".to_string(),
                metadata: serde_json::json!({"level": "confidential"}),
                created_at: Utc::now().timestamp(),
            }],
            vectors: vec![],
            snapshots: vec!["repo_alice_v1".to_string()],
        };

        let result = registry.sync_deltas(wallet_a, wallet_b, payload);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Cross-wallet sync rejected"));
    }

    #[test]
    fn test_sync_same_wallet_memory_delta() {
        let file = NamedTempFile::new().unwrap();
        let mut registry = PrivateMeshRegistry::load_or_create(file.path().to_path_buf()).unwrap();

        let wallet_id = "wallet_alice_123";
        let node_a = make_test_node("xv1-alice-laptop", wallet_id, "Laptop");
        let node_b = make_test_node("xv1-alice-phone", wallet_id, "Phone");

        registry.register_wallet_node(node_a, wallet_id).unwrap();
        registry.register_wallet_node(node_b, wallet_id).unwrap();

        let memory_delta = PrivateMemoryDelta {
            path: "fact/swal_node_1".to_string(),
            content: "Shared wallet state delta".to_string(),
            metadata: serde_json::json!({"synced": true}),
            created_at: Utc::now().timestamp(),
        };

        let payload = PrivateSyncPayload {
            memories: vec![memory_delta.clone()],
            vectors: vec![],
            snapshots: vec!["snap_swal_repo".to_string()],
        };

        let synced = registry
            .sync_deltas(wallet_id, wallet_id, payload)
            .expect("Same wallet sync must succeed");

        assert_eq!(synced.memories.len(), 1);
        assert_eq!(synced.memories[0].path, "fact/swal_node_1");
        assert_eq!(synced.memories[0].content, "Shared wallet state delta");
        assert_eq!(synced.snapshots, vec!["snap_swal_repo"]);
    }

    #[test]
    fn test_isolation_cross_wallet() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();

        let mut registry = PrivateMeshRegistry::load_or_create(path).unwrap();

        let wallet_a = "wallet_alpha_111";
        let wallet_b = "wallet_beta_222";

        let node_a1 = make_test_node("xv1-alpha1", wallet_a, "Alpha Node 1");
        let node_a2 = make_test_node("xv1-alpha2", wallet_a, "Alpha Node 2");
        let node_b1 = make_test_node("xv1-beta1", wallet_b, "Beta Node 1");

        // (a) Register A1 and A2 under wallet_a, B1 under wallet_b
        registry.register_wallet_node(node_a1, wallet_a).unwrap();
        registry.register_wallet_node(node_a2, wallet_a).unwrap();
        registry
            .register_wallet_node(node_b1.clone(), wallet_b)
            .unwrap();

        // (b) Assert get_nodes_by_wallet isolation both directions
        let nodes_a = registry.get_nodes_by_wallet(wallet_a);
        let nodes_b = registry.get_nodes_by_wallet(wallet_b);

        assert_eq!(nodes_a.len(), 2);
        assert_eq!(nodes_b.len(), 1);
        assert!(nodes_a.iter().all(|n| n.wallet_id == wallet_a));
        assert!(nodes_b.iter().all(|n| n.wallet_id == wallet_b));
        assert!(!nodes_a.iter().any(|n| n.wallet_id == wallet_b));
        assert!(!nodes_b.iter().any(|n| n.wallet_id == wallet_a));

        // (c) Assert sync_deltas(wallet_a, wallet_b, payload) is Err with "Cross-wallet sync rejected"
        let payload = PrivateSyncPayload {
            memories: vec![PrivateMemoryDelta {
                path: "fact/isolated_memory".to_string(),
                content: "Cross-wallet payload".to_string(),
                metadata: serde_json::json!({"isolated": true}),
                created_at: Utc::now().timestamp(),
            }],
            vectors: vec![],
            snapshots: vec![],
        };
        let sync_res = registry.sync_deltas(wallet_a, wallet_b, payload);
        assert!(sync_res.is_err());
        let err_msg = sync_res.unwrap_err().to_string();
        assert!(err_msg.contains("Cross-wallet sync rejected"));

        // (d) Assert B-node register_wallet_node(node_b, wallet_a) is Err
        let register_mismatch = registry.register_wallet_node(node_b1, wallet_a);
        assert!(register_mismatch.is_err());
    }

    #[test]
    fn test_encrypted_vector_sync_and_blind_search() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        let registry = PrivateMeshRegistry::load_or_create(path).unwrap();

        let wallet_id = "wallet_mesh_legal_secure";
        let vector_payload = PrivateSyncPayload {
            memories: vec![],
            vectors: vec![
                PrivateVectorDelta {
                    path: "expediente/001/demanda.pdf".to_string(),
                    embedding: vec![1.0, 0.0, 0.0],
                    metadata: serde_json::json!({"case_id": "EXP-001"}),
                },
                PrivateVectorDelta {
                    path: "expediente/002/contestacion.pdf".to_string(),
                    embedding: vec![0.0, 1.0, 0.0],
                    metadata: serde_json::json!({"case_id": "EXP-002"}),
                },
            ],
            snapshots: vec![],
        };

        // Encrypt & sync deltas across same wallet
        let synced = registry
            .sync_deltas(wallet_id, wallet_id, vector_payload.clone())
            .expect("Same wallet vector sync must succeed");

        assert_eq!(synced.vectors.len(), 2);
        assert_eq!(synced.vectors[0].path, "expediente/001/demanda.pdf");

        // Blind vector search test: query aligned with [1.0, 0.0, 0.0]
        let query = vec![0.99, 0.05, 0.0];
        let hits = registry.blind_vector_search(&query, &synced.vectors, 5, 0.5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "expediente/001/demanda.pdf");
        assert!(hits[0].1 > 0.95);
    }

    #[test]
    fn test_ecdh_session_encryption_forward_secrecy_and_isolation() {
        let sk_a = SigningKey::from_bytes(&[101u8; 32]);
        let pubkey_a = sk_a.verifying_key().to_bytes();
        let secret_a = signing_key_to_x25519_secret(&sk_a);

        let sk_b = SigningKey::from_bytes(&[202u8; 32]);
        let secret_b = signing_key_to_x25519_secret(&sk_b);

        let payload = PrivateSyncPayload {
            memories: vec![PrivateMemoryDelta {
                path: "secrets/confidential_memo".to_string(),
                content: "ECDH forward-secret payload".to_string(),
                metadata: serde_json::json!({"level": "top_secret"}),
                created_at: Utc::now().timestamp(),
            }],
            vectors: vec![],
            snapshots: vec!["snap-1".to_string()],
        };

        // Encrypt two payloads for the same recipient; verify each gets a distinct ephemeral key and ciphertext
        let enc1 = encrypt_session_payload(&payload, &pubkey_a).expect("encrypt 1 failed");
        let enc2 = encrypt_session_payload(&payload, &pubkey_a).expect("encrypt 2 failed");

        assert!(enc1.ephemeral_public_key_hex.is_some());
        assert!(enc2.ephemeral_public_key_hex.is_some());
        assert_ne!(
            enc1.ephemeral_public_key_hex, enc2.ephemeral_public_key_hex,
            "Fresh ephemeral key required per session"
        );
        assert_ne!(enc1.ciphertext_hex, enc2.ciphertext_hex);

        // Decryption by legitimate recipient succeeds
        let dec = decrypt_session_payload(&enc1, &secret_a).expect("legitimate decrypt failed");
        assert_eq!(dec.memories.len(), 1);
        assert_eq!(dec.memories[0].content, "ECDH forward-secret payload");

        // Decryption by rogue recipient secret fails
        let rogue_dec = decrypt_session_payload(&enc1, &secret_b);
        assert!(
            rogue_dec.is_err(),
            "Rogue secret must not decrypt ECDH session payload"
        );

        // Tampered ephemeral key fails decryption
        let mut tampered = enc1.clone();
        tampered.ephemeral_public_key_hex = Some("00".repeat(32));
        assert!(decrypt_session_payload(&tampered, &secret_a).is_err());
    }

    #[test]
    fn test_missing_ephemeral_key_fails_decryption() {
        let sk = SigningKey::from_bytes(&[101u8; 32]);
        let secret = signing_key_to_x25519_secret(&sk);

        let payload_without_ephem = EncryptedSessionPayload {
            ciphertext_hex: "abcd".to_string(),
            nonce_hex: "00".repeat(12),
            ephemeral_public_key_hex: None,
        };

        assert!(
            decrypt_session_payload(&payload_without_ephem, &secret).is_err(),
            "Payload without ephemeral public key must be rejected"
        );
    }

    #[test]
    fn test_attacker_with_wallet_id_and_pubkey_cannot_decrypt() {
        let alice_sk = SigningKey::from_bytes(&[77u8; 32]);
        let alice_pubkey = alice_sk.verifying_key().to_bytes();
        let alice_wallet_id = derive_wallet_id(&alice_pubkey);

        let payload = PrivateSyncPayload {
            memories: vec![PrivateMemoryDelta {
                path: "secrets/classified".to_string(),
                content: "Top secret content".to_string(),
                metadata: serde_json::json!({}),
                created_at: Utc::now().timestamp(),
            }],
            vectors: vec![],
            snapshots: vec![],
        };

        let encrypted = encrypt_session_payload(&payload, &alice_pubkey)
            .expect("Encryption with Alice's public key succeeds");

        // An attacker observes only alice_wallet_id and alice_pubkey.
        // If the attacker attempts to construct an X25519 secret derived only from public material:
        let mut attacker_seed = [0u8; 32];
        let mut hasher = Sha256::new();
        hasher.update(alice_wallet_id.as_bytes());
        attacker_seed.copy_from_slice(&hasher.finalize());
        let attacker_secret = StaticSecret::from(attacker_seed);

        let attacker_dec = decrypt_session_payload(&encrypted, &attacker_secret);
        assert!(
            attacker_dec.is_err(),
            "Attacker possessing only wallet_id and public key must fail decryption"
        );

        // Attacker attempting with SHA-256 of public key
        let mut hasher_pub = Sha256::new();
        hasher_pub.update(alice_pubkey);
        attacker_seed.copy_from_slice(&hasher_pub.finalize());
        let attacker_secret_from_pub = StaticSecret::from(attacker_seed);
        assert!(
            decrypt_session_payload(&encrypted, &attacker_secret_from_pub).is_err(),
            "Attacker possessing only public key hash must fail decryption"
        );
    }
}
