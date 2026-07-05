//! Push and pull logic for memory sync.
//!
//! Provides functions to:
//! - Collect local changes since a timestamp
//! - Push changes to a remote peer
//! - Pull changes from a remote peer (with timestamp filtering)

use std::time::SystemTime;

use anyhow::Result;

use crate::memory::store::MemoryStore;

use super::merge::{chunk_hash, serialise_chunk};
use super::{ChunkDiff, DiffAction};

/// Collect all chunks in a workspace that have been modified since `since`.
///
/// Returns a vec of `ChunkDiff` (Add/Update actions with data).
pub async fn collect_changes_since(
    store: &dyn MemoryStore,
    workspace_id: &str,
    since: SystemTime,
) -> Result<Vec<ChunkDiff>> {
    let records = store.list(workspace_id).await?;
    let since_dt: chrono::DateTime<chrono::Utc> = since.into();

    let mut diffs = Vec::new();
    for rec in &records {
        if rec.updated_at > since_dt {
            let hash = chunk_hash(rec);
            let data = serialise_chunk(rec)?;
            let timestamp = rec.updated_at.into();
            diffs.push(ChunkDiff {
                chunk_hash: hash,
                namespace: workspace_id.to_string(),
                action: DiffAction::Update,
                data: Some(data),
                timestamp,
            });
        }
    }
    Ok(diffs)
}

/// Convert manifest entries into serialised ChunkDiffs for transmission to a peer.
///
/// Fetches each entry's record from the store by scanning the workspace.
pub async fn entries_as_push_diffs(
    store: &dyn MemoryStore,
    entries: &[super::ManifestEntry],
) -> Result<Vec<ChunkDiff>> {
    let mut diffs = Vec::new();
    for entry in entries {
        // Fetch records from store for this namespace
        let records = store.list(&entry.namespace).await?;
        let matched: Vec<_> = records
            .iter()
            .filter(|r| chunk_hash(r) == entry.chunk_hash)
            .collect();

        if let Some(rec) = matched.first() {
            let data = serialise_chunk(rec)?;
            diffs.push(ChunkDiff {
                chunk_hash: entry.chunk_hash.clone(),
                namespace: entry.namespace.clone(),
                action: DiffAction::Update,
                data: Some(data),
                timestamp: rec.updated_at.into(),
            });
        } else {
            // Record not found locally — push as Delete
            diffs.push(ChunkDiff {
                chunk_hash: entry.chunk_hash.clone(),
                namespace: entry.namespace.clone(),
                action: DiffAction::Delete,
                data: None,
                timestamp: std::time::SystemTime::now(),
            });
        }
    }
    Ok(diffs)
}

/// Serialise a workspace's entire record set as a list of ChunkDiffs.
///
/// Used for full-store sync (initial sync, or when manifests diverge too far).
pub async fn collect_all_chunks(
    store: &dyn MemoryStore,
    workspace_id: &str,
) -> Result<Vec<ChunkDiff>> {
    let records = store.list(workspace_id).await?;
    let mut diffs = Vec::new();
    for rec in &records {
        let hash = chunk_hash(rec);
        let data = serialise_chunk(rec)?;
        let timestamp = rec.updated_at.into();
        diffs.push(ChunkDiff {
            chunk_hash: hash,
            namespace: workspace_id.to_string(),
            action: DiffAction::Update,
            data: Some(data),
            timestamp,
        });
    }
    Ok(diffs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::MemoryRecord;
    use crate::memory::sync::manifest::tests::TestStore;
    use chrono::{TimeDelta, Utc};
    use std::sync::Arc;

    fn make_record(
        id: &str,
        workspace: &str,
        content: &str,
        updated_at: chrono::DateTime<Utc>,
        revision: u64,
    ) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            workspace_id: workspace.to_string(),
            path: format!("test/{}", id),
            content: content.to_string(),
            metadata: serde_json::Value::Null,
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
        }
    }

    #[tokio::test]
    async fn test_no_changes_since_future() {
        let store = Arc::new(TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        let future = SystemTime::now() + std::time::Duration::from_secs(3600);
        let diffs = collect_changes_since(&*store, "episodic", future)
            .await
            .unwrap();
        assert!(diffs.is_empty());
    }

    #[tokio::test]
    async fn test_collect_changes_since() {
        let store = Arc::new(TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        let now = Utc::now();
        let rec = make_record("c1", "episodic", "some content", now, 1);

        // Must use `store.put()` to make it available via `list()`
        store.put(rec).await.unwrap();

        let hour_ago = now - TimeDelta::hours(1);
        let since = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(hour_ago.timestamp() as u64);
        let diffs = collect_changes_since(&*store, "episodic", since)
            .await
            .unwrap();
        assert_eq!(diffs.len(), 1, "should find 1 changed chunk");
    }
}
