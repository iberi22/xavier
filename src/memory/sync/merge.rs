//! LWW (Last Writer Wins) merge resolver for memory chunks.
//!
//! Conflict resolution rules:
//! 1. Higher `updated_at` timestamp wins.
//! 2. Same timestamp → higher `node_id` (lexicographic) wins.
//! 3. Delete always wins over Add/Update (tombstone semantics).

use anyhow::Result;
use sha2::{Digest, Sha256};

use crate::memory::store::{MemoryRecord, MemoryStore};

use super::{ChunkDiff, DiffAction};

/// Apply a set of incoming ChunkDiffs to the local store using LWW resolution.
///
/// `conflicts` is incremented for each chunk where both sides had data
/// and the local revision was overridden.
pub async fn apply_changes_received(
    store: &dyn MemoryStore,
    diffs: &[ChunkDiff],
    conflicts: &mut u64,
) -> Result<()> {
    for diff in diffs {
        match diff.action {
            DiffAction::Add | DiffAction::Update => {
                let data = match &diff.data {
                    Some(d) => d,
                    None => continue,
                };
                // Deserialise the chunk data into a MemoryRecord
                let incoming: MemoryRecord = match serde_json::from_slice(data) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(
                            "apply_changes_received: cannot deserialise chunk {}: {e}",
                            diff.chunk_hash
                        );
                        continue;
                    }
                };

                // Check if we already have this record
                let existing = store.get(&incoming.workspace_id, &incoming.path).await?;

                match existing {
                    None => {
                        // No local record → accept incoming
                        store.put(incoming).await?;
                    }
                    Some(local) => {
                        // LWW: keep the newer one
                        if incoming.updated_at > local.updated_at {
                            *conflicts += 1;
                            store.put(incoming).await?;
                        } else if incoming.updated_at == local.updated_at {
                            // Same timestamp → tie-break by node_id hash
                            let local_node = lww_node_id(&local);
                            let incoming_node = lww_node_id(&incoming);
                            if incoming_node > local_node {
                                *conflicts += 1;
                                store.put(incoming).await?;
                            }
                        }
                        // else local is newer → keep it (no-op)
                    }
                }
            }
            DiffAction::Delete => {
                // Delete always wins (tombstone)
                let workspace_id = &diff.namespace;
                let path = &diff.chunk_hash; // use hash as path for deletion
                store.delete(workspace_id, path).await?;
            }
        }
    }
    Ok(())
}

/// Derive a stable lexicographic node id from a MemoryRecord's metadata.
fn lww_node_id(record: &MemoryRecord) -> String {
    // Use the record's id as a proxy for node_id.
    // In production, each peer attaches its node_id to metadata upon creation.
    record
        .metadata
        .get("node_id")
        .and_then(|v| v.as_str())
        .unwrap_or(&record.id)
        .to_string()
}

/// Serialise a MemoryRecord into bytes for transport.
pub fn serialise_chunk(record: &MemoryRecord) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(record)?)
}

/// Deserialise a chunk payload into a MemoryRecord.
pub fn deserialise_chunk(data: &[u8]) -> Result<MemoryRecord> {
    Ok(serde_json::from_slice(data)?)
}

/// Extract the manifest-relevant hash for a record.
pub fn chunk_hash(record: &MemoryRecord) -> String {
    let mut hasher = Sha256::new();
    hasher.update(record.path.as_bytes());
    hasher.update(record.revision.to_le_bytes());
    hasher.update(record.updated_at.timestamp().to_le_bytes());
    crate::crypto::hex_encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::MemoryRecord;
    use chrono::{TimeDelta, Utc};
    use std::sync::Arc;

    // Reuse TestStore from manifest.rs
    use crate::memory::sync::manifest::tests as manifest_tests;

    fn make_record(
        id: &str,
        workspace_id: &str,
        content: &str,
        updated_at: chrono::DateTime<Utc>,
        revision: u64,
        node_id: &str,
    ) -> MemoryRecord {
        let mut meta = serde_json::Map::new();
        meta.insert(
            "node_id".to_string(),
            serde_json::Value::String(node_id.to_string()),
        );
        MemoryRecord {
            id: id.to_string(),
            workspace_id: workspace_id.to_string(),
            path: format!("test/{}", id),
            content: content.to_string(),
            metadata: serde_json::Value::Object(meta),
            embedding: Vec::new(),
            created_at: updated_at,
            updated_at,
            revision,
            primary: true,
            parent_id: None,
            cluster_id: None,
            level: crate::memory::schema::MemoryLevel::Raw,
            relation: None,
            clearance: crate::memory::schema::ClearanceLevel::Unclassified,
            revisions: Vec::new(),
            encrypted_dek: None,
            content_iv: None,
            metadata_iv: None,
            score: 0.0,
            deleted_at: None,
        }
    }

    #[tokio::test]
    async fn test_apply_empty_diffs() {
        let store = Arc::new(manifest_tests::TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        let mut conflicts = 0;
        apply_changes_received(&*store, &[], &mut conflicts)
            .await
            .unwrap();
        assert_eq!(conflicts, 0);
    }

    #[tokio::test]
    async fn test_lww_newer_timestamp_wins() {
        let store = Arc::new(manifest_tests::TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        // Put a record with old timestamp
        let old = make_record("r1", "episodic", "old content", Utc::now(), 1, "node_a");
        store.put(old).await.unwrap();

        // Incoming record has newer timestamp
        let newer = make_record(
            "r1",
            "episodic",
            "new content",
            Utc::now() + TimeDelta::seconds(10),
            2,
            "node_b",
        );
        let serialized = serialise_chunk(&newer).unwrap();
        let diff = ChunkDiff {
            chunk_hash: chunk_hash(&newer),
            namespace: "episodic".to_string(),
            action: DiffAction::Update,
            data: Some(serialized),
            timestamp: std::time::SystemTime::now(),
        };

        let mut conflicts = 0;
        apply_changes_received(&*store, &[diff], &mut conflicts)
            .await
            .unwrap();
        assert_eq!(conflicts, 1, "older local was overridden → conflict");

        let fetched = store
            .get("episodic", "test/r1")
            .await
            .unwrap()
            .expect("record should exist");
        assert_eq!(fetched.content, "new content", "newer content should win");
    }

    #[tokio::test]
    async fn test_lww_same_timestamp_higher_node_id_wins() {
        let store = Arc::new(manifest_tests::TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        let now = Utc::now();
        let local = make_record("r2", "semantic", "from node_a", now, 1, "A");
        store.put(local).await.unwrap();

        let incoming = make_record("r2", "semantic", "from node_b", now, 1, "B");
        let serialized = serialise_chunk(&incoming).unwrap();
        let diff = ChunkDiff {
            chunk_hash: chunk_hash(&incoming),
            namespace: "semantic".to_string(),
            action: DiffAction::Update,
            data: Some(serialized),
            timestamp: std::time::SystemTime::now(),
        };

        let mut conflicts = 0;
        apply_changes_received(&*store, &[diff], &mut conflicts)
            .await
            .unwrap();
        assert_eq!(conflicts, 1);

        let fetched = store
            .get("semantic", "test/r2")
            .await
            .unwrap()
            .expect("record should exist");
        assert_eq!(
            fetched.content, "from node_b",
            "B > A lexicographically → B wins"
        );
    }

    #[tokio::test]
    async fn test_delete_propagation() {
        let store = Arc::new(manifest_tests::TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        let rec = make_record("del1", "working", "to delete", Utc::now(), 1, "node_a");
        store.put(rec).await.unwrap();

        let diff = ChunkDiff {
            chunk_hash: "del1".to_string(),
            namespace: "working".to_string(),
            action: DiffAction::Delete,
            data: None,
            timestamp: std::time::SystemTime::now(),
        };

        let mut conflicts = 0;
        apply_changes_received(&*store, &[diff], &mut conflicts)
            .await
            .unwrap();

        let fetched = store.get("working", "test/del1").await.unwrap();
        assert!(fetched.is_none(), "record should be deleted");
    }
}
