//! XMesh-Sync v1 Protocol Types
//!
//! This module defines the data structures used in the Xavier Mesh sync protocol.
//! All types are designed for JSON serialization over HTTP.

use crate::mesh::node::NodeId;
use crate::session::sharing::SessionBundle;
use serde::{Deserialize, Serialize};

/// Initial handshake sent by a node to a peer.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MeshHandshake {
    pub node_id: NodeId,
    pub public_key_hex: String,
    pub xavier_version: String,
    pub capabilities: Vec<String>,
    pub timestamp: i64,
    pub nonce: String,
    pub signature_hex: String,
    pub pairing_secret: Option<String>,
}

/// Response to a [`MeshHandshake`].
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MeshHandshakeResponse {
    pub accepted: bool,
    pub node_id: NodeId,
    pub public_key_hex: String,
    pub reason: Option<String>,
}

/// A manifest of chunks available on a node.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MeshManifest {
    pub node_id: NodeId,
    pub chunks: Vec<ChunkRef>,
    pub generated_at: i64,
}

/// Reference to a specific memory chunk.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChunkRef {
    pub hash: String,
    pub document_count: usize,
    pub created_at: i64,
}

/// Request for specific chunks from a peer.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MeshSyncRequest {
    pub requesting_node_id: NodeId,
    pub wanted_hashes: Vec<String>,
    pub timestamp: i64,
    pub nonce: String,
    pub signature_hex: String,
}

/// Result of a sync operation.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MeshSyncResult {
    pub synced_chunks: Vec<String>,
    pub failed_chunks: Vec<String>,
    pub duration_ms: u64,
}

/// Request to share a session bundle with a peer.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MeshSessionShare {
    pub sender_node_id: NodeId,
    pub bundle: SessionBundle,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handshake_serialization() {
        let handshake = MeshHandshake {
            node_id: NodeId("xv1-test".to_string()),
            public_key_hex: "01020304".to_string(),
            xavier_version: "0.1.0".to_string(),
            capabilities: vec!["sync-v1".to_string()],
            timestamp: 123456789,
            nonce: "nonce123".to_string(),
            signature_hex: "sig123".to_string(),
        };

        let json = serde_json::to_string(&handshake).unwrap();
        let deserialized: MeshHandshake = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.node_id, handshake.node_id);
        assert_eq!(deserialized.public_key_hex, handshake.public_key_hex);
    }

    #[test]
    fn test_manifest_serialization() {
        let manifest = MeshManifest {
            node_id: NodeId("xv1-test".to_string()),
            chunks: vec![ChunkRef {
                hash: "hash1".to_string(),
                document_count: 5,
                created_at: 123456789,
            }],
            generated_at: 123456789,
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let deserialized: MeshManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.node_id, manifest.node_id);
        assert_eq!(deserialized.chunks.len(), 1);
        assert_eq!(deserialized.chunks[0].hash, "hash1");
    }
}
