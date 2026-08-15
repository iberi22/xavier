//! Private Mesh Registry by Wallet Keys
//!
//! Handles registration and isolation of nodes belonging to the same wallet,
//! ensuring private mesh security.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

use crate::mesh::node::NodeId;

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

/// The payload exchanged between nodes of the same wallet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PrivateSyncPayload {
    pub memories: Vec<PrivateMemoryDelta>,
    pub snapshots: Vec<String>,
}

/// Encrypted session payload wrapper for transport over Iroh.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedSessionPayload {
    pub ciphertext_hex: String,
    pub nonce_hex: String,
}

/// Derives a 32-byte AES key from a wallet ID for session encryption.
pub fn derive_wallet_session_key(wallet_id: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"private_mesh_session:");
    hasher.update(wallet_id.as_bytes());
    hasher.finalize().into()
}

/// Encrypts a [`PrivateSyncPayload`] using the session key derived from `wallet_id`.
pub fn encrypt_session_payload(
    payload: &PrivateSyncPayload,
    wallet_id: &str,
) -> Result<EncryptedSessionPayload> {
    let key = derive_wallet_session_key(wallet_id);
    let nonce = crate::crypto::encryption::NonceBytes::generate();
    let json_bytes = serde_json::to_vec(payload)?;
    let blob = crate::crypto::encryption::encrypt_data(&json_bytes, &key, &nonce)
        .map_err(|e| anyhow!("Encryption failed: {:?}", e))?;

    Ok(EncryptedSessionPayload {
        ciphertext_hex: crate::crypto::hex_encode(&blob.ciphertext),
        nonce_hex: crate::crypto::hex_encode(&blob.nonce),
    })
}

/// Decrypts an [`EncryptedSessionPayload`] using the session key derived from `wallet_id`.
pub fn decrypt_session_payload(
    encrypted: &EncryptedSessionPayload,
    wallet_id: &str,
) -> Result<PrivateSyncPayload> {
    let key = derive_wallet_session_key(wallet_id);
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

/// registry to load/save JSON data for private mesh nodes.
#[derive(Debug)]
pub struct PrivateMeshRegistry {
    nodes: Vec<WalletNode>,
    file_path: PathBuf,
}

impl PrivateMeshRegistry {
    /// Create a new empty registry with a specified file path (without loading).
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            nodes: Vec::new(),
            file_path,
        }
    }

    /// Load the registry from the specified JSON file, or create it if it does not exist or is empty.
    pub fn load_or_create(file_path: PathBuf) -> Result<Self> {
        if file_path.exists() && std::fs::metadata(&file_path)?.len() > 0 {
            let data = std::fs::read_to_string(&file_path)?;
            let nodes: Vec<WalletNode> = serde_json::from_str(&data)?;
            Ok(Self { nodes, file_path })
        } else {
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let registry = Self {
                nodes: Vec::new(),
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
        let data = serde_json::to_string_pretty(&self.nodes)?;
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

        // Upsert logic: update if exists, otherwise push
        if let Some(pos) = self.nodes.iter().position(|n| n.node_id == node.node_id) {
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

        // Encrypt with session key derived from wallet_id
        let encrypted = encrypt_session_payload(&payload, source_wallet)?;

        // Decrypt on target node with same wallet_id
        let decrypted = decrypt_session_payload(&encrypted, target_wallet)?;

        Ok(decrypted)
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
    fn test_isolation_cross_wallet() {
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
}
