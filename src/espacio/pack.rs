//! Pack RAG — .swalpack export/import for Spaces (T-04)
//!
//! Pack format (v1): JSON (CBOR + zstd in follow-up). Contains manifest +
//! memories + vectors. Import validates ML-DSA signature stub, content hash
//! and dedup 0.92 threshold placeholder.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Manifest for a pack
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackManifest {
    pub space_id: String,
    pub created_at: DateTime<Utc>,
    pub content_hash: String,
    pub vector_dim: usize,
    pub memory_count: usize,
    pub version: String,
}

/// Single memory entry in a pack
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackMemory {
    pub id: String,
    pub path: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// A pack ready to export or import
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pack {
    pub manifest: PackManifest,
    pub memories: Vec<PackMemory>,
    /// Optional ML-DSA signature hex over content_hash
    pub signature: Option<String>,
}

impl Pack {
    /// Compute SHA256 hex of serialized memories (deterministic)
    pub fn compute_hash(memories: &[PackMemory]) -> String {
        let mut hasher = Sha256::new();
        for m in memories {
            hasher.update(m.id.as_bytes());
            hasher.update(m.path.as_bytes());
            hasher.update(m.content.as_bytes());
        }
        crate::crypto::hex_encode(hasher.finalize())
    }

    /// Create a new pack from memories
    pub fn new(space_id: String, memories: Vec<PackMemory>, vector_dim: usize) -> Self {
        let hash = Self::compute_hash(&memories);
        let manifest = PackManifest {
            space_id: space_id.clone(),
            created_at: Utc::now(),
            content_hash: hash,
            vector_dim,
            memory_count: memories.len(),
            version: "1".into(),
        };
        Self {
            manifest,
            memories,
            signature: None,
        }
    }

    /// Serialize to JSON bytes (CBOR + zstd will replace this in v2)
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| anyhow!(e))
    }

    /// Deserialize from JSON bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let pack: Self = serde_json::from_slice(bytes).map_err(|e| anyhow!(e))?;
        // Verify hash matches content
        let expected = Self::compute_hash(&pack.memories);
        if pack.manifest.content_hash != expected {
            return Err(anyhow!(
                "content hash mismatch: manifest {} vs computed {}",
                pack.manifest.content_hash,
                expected
            ));
        }
        if pack.manifest.memory_count != pack.memories.len() {
            return Err(anyhow!("memory count mismatch"));
        }
        Ok(pack)
    }

    /// Attach ML-DSA signature (hex) over content_hash
    pub fn attach_signature(&mut self, sig_hex: String) {
        self.signature = Some(sig_hex);
    }

    /// Verify signature stub (true if no signature required or matches expected)
    pub fn verify_signature(&self, expected_sig: Option<&str>) -> bool {
        match (&self.signature, expected_sig) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }
}

/// Dedup threshold placeholder (0.92 cosine). Stub uses exact content equality for now.
pub fn is_duplicate(content_a: &str, content_b: &str) -> bool {
    content_a == content_b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_memories() -> Vec<PackMemory> {
        vec![
            PackMemory {
                id: "m1".into(),
                path: "a/b".into(),
                content: "hello world".into(),
                created_at: Utc::now(),
            },
            PackMemory {
                id: "m2".into(),
                path: "a/c".into(),
                content: "foo bar".into(),
                created_at: Utc::now(),
            },
        ]
    }

    #[test]
    fn pack_roundtrip() {
        let pack = Pack::new("esp_a".into(), sample_memories(), 1536);
        let bytes = pack.to_bytes().unwrap();
        let decoded = Pack::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.manifest.space_id, "esp_a");
        assert_eq!(decoded.memories.len(), 2);
        assert_eq!(decoded.manifest.vector_dim, 1536);
    }

    #[test]
    fn hash_mismatch_rejected() {
        let mut pack = Pack::new("esp_a".into(), sample_memories(), 1536);
        // tamper manifest hash
        pack.manifest.content_hash = "bad".into();
        let bytes = serde_json::to_vec(&pack).unwrap();
        assert!(Pack::from_bytes(&bytes).is_err());
    }

    #[test]
    fn dedup_stub() {
        assert!(is_duplicate("same", "same"));
        assert!(!is_duplicate("a", "b"));
    }

    #[test]
    fn signature_attach_verify() {
        let mut pack = Pack::new("esp_a".into(), sample_memories(), 1536);
        pack.attach_signature("abcd".into());
        assert!(pack.verify_signature(Some("abcd")));
        assert!(!pack.verify_signature(Some("other")));
        assert!(!pack.verify_signature(None));
    }
}
