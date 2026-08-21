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
                            // Reuse the local row id so `put`'s INSERT OR REPLACE
                            // updates in place instead of inserting a duplicate
                            // (same pattern as SSP canonical paths).
                            let mut incoming = incoming;
                            incoming.id = local.id;
                            store.put(incoming).await?;
                        } else if incoming.updated_at == local.updated_at {
                            // Same timestamp → tie-break by node_id hash
                            let local_node = lww_node_id(&local);
                            let incoming_node = lww_node_id(&incoming);
                            if incoming_node > local_node {
                                *conflicts += 1;
                                let mut incoming = incoming;
                                incoming.id = local.id;
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
                // Use the actual record path/id, not the chunk_hash.
                // chunk_hash is a SHA-256 hash which is meaningless as a path.
                let path = match &diff.record_path {
                    Some(p) => p.as_str(),
                    None => {
                        tracing::warn!(
                            "apply_changes_received: delete diff for chunk {} has no record_path, skipping",
                            diff.chunk_hash
                        );
                        continue;
                    }
                };
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
            clearance: crate::security::clearance::ClearanceLevel::Unclassified,
            revisions: Vec::new(),
            encrypted_dek: None,
            content_iv: None,
            metadata_iv: None,
            score: 0.0,
            deleted_at: None,
            ..Default::default()
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
            record_path: None,
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
            record_path: None,
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
            chunk_hash: "some_hash".to_string(),
            namespace: "working".to_string(),
            action: DiffAction::Delete,
            data: None,
            timestamp: std::time::SystemTime::now(),
            record_path: Some("test/del1".to_string()),
        };

        let mut conflicts = 0;
        apply_changes_received(&*store, &[diff], &mut conflicts)
            .await
            .unwrap();

        let fetched = store.get("working", "test/del1").await.unwrap();
        assert!(fetched.is_none(), "record should be deleted");
    }

    #[tokio::test]
    async fn test_delete_skipped_without_record_path() {
        let store = Arc::new(manifest_tests::TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        let rec = make_record("del2", "working", "should survive", Utc::now(), 1, "node_a");
        store.put(rec).await.unwrap();

        // Delete diff WITHOUT record_path — should be skipped, not crash
        let diff = ChunkDiff {
            chunk_hash: "some_hash".to_string(),
            namespace: "working".to_string(),
            action: DiffAction::Delete,
            data: None,
            timestamp: std::time::SystemTime::now(),
            record_path: None,
        };

        let mut conflicts = 0;
        apply_changes_received(&*store, &[diff], &mut conflicts)
            .await
            .unwrap();

        // Record should survive because we skipped the delete (no path info)
        let fetched = store.get("working", "test/del2").await.unwrap();
        assert!(
            fetched.is_some(),
            "record should survive when delete has no record_path"
        );
    }

    #[tokio::test]
    async fn test_delete_only_affects_target_record() {
        let store = Arc::new(manifest_tests::TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        // Put two records in the same workspace
        let rec1 = make_record("keep", "workspace1", "keep this", Utc::now(), 1, "node_a");
        let rec2 = make_record(
            "remove",
            "workspace1",
            "remove this",
            Utc::now(),
            1,
            "node_a",
        );
        store.put(rec1).await.unwrap();
        store.put(rec2).await.unwrap();

        // Delete only rec2
        let diff = ChunkDiff {
            chunk_hash: "hash_for_rec2".to_string(),
            namespace: "workspace1".to_string(),
            action: DiffAction::Delete,
            data: None,
            timestamp: std::time::SystemTime::now(),
            record_path: Some("test/remove".to_string()),
        };

        let mut conflicts = 0;
        apply_changes_received(&*store, &[diff], &mut conflicts)
            .await
            .unwrap();

        // rec1 should survive
        let kept = store.get("workspace1", "test/keep").await.unwrap();
        assert!(kept.is_some(), "unrelated record should not be deleted");
        assert_eq!(kept.unwrap().content, "keep this");

        // rec2 should be gone
        let removed = store.get("workspace1", "test/remove").await.unwrap();
        assert!(removed.is_none(), "target record should be deleted");
    }

    #[tokio::test]
    async fn test_delete_nonexistent_record_is_noop() {
        let store = Arc::new(manifest_tests::TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        // Delete a record that doesn't exist — should not error
        let diff = ChunkDiff {
            chunk_hash: "hash".to_string(),
            namespace: "empty_ws".to_string(),
            action: DiffAction::Delete,
            data: None,
            timestamp: std::time::SystemTime::now(),
            record_path: Some("nonexistent/path".to_string()),
        };

        let mut conflicts = 0;
        apply_changes_received(&*store, &[diff], &mut conflicts)
            .await
            .unwrap();
        // No panic, no error — just a no-op delete
    }

    #[tokio::test]
    async fn test_mixed_add_and_delete() {
        let store = Arc::new(manifest_tests::TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        // Put a record to delete
        let old = make_record("gone", "ws", "delete me", Utc::now(), 1, "node_a");
        store.put(old).await.unwrap();

        // Create an Add diff for a new record
        let new_rec = make_record(
            "new_one",
            "ws",
            "brand new",
            Utc::now() + TimeDelta::seconds(5),
            1,
            "node_b",
        );
        let serialized = serialise_chunk(&new_rec).unwrap();

        let diffs = vec![
            ChunkDiff {
                chunk_hash: "hash_gone".to_string(),
                namespace: "ws".to_string(),
                action: DiffAction::Delete,
                data: None,
                timestamp: std::time::SystemTime::now(),
                record_path: Some("test/gone".to_string()),
            },
            ChunkDiff {
                chunk_hash: chunk_hash(&new_rec),
                namespace: "ws".to_string(),
                action: DiffAction::Add,
                data: Some(serialized),
                timestamp: std::time::SystemTime::now(),
                record_path: None,
            },
        ];

        let mut conflicts = 0;
        apply_changes_received(&*store, &diffs, &mut conflicts)
            .await
            .unwrap();

        assert!(
            store.get("ws", "test/gone").await.unwrap().is_none(),
            "old record deleted"
        );
        assert_eq!(
            store
                .get("ws", "test/new_one")
                .await
                .unwrap()
                .unwrap()
                .content,
            "brand new",
            "new record added"
        );
    }
}
