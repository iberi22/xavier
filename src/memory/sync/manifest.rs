//! Manifest building and reconciliation for memory sync.
//!
//! A manifest is a compact snapshot of all chunks known to a store,
//! used to determine what has changed between two peers.

use anyhow::Result;

use crate::memory::store::MemoryStore;

use super::{Manifest, ManifestEntry};

/// Build a full manifest from a MemoryStore.
///
/// Enumerates every record in every workspace and produces a compact
/// entry for each, suitable for transmission between peers.
pub async fn build_manifest(store: &dyn MemoryStore) -> Result<Manifest> {
    let workspaces = store.list_workspaces().await?;
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for workspace_id in &workspaces {
        let records = store.list(workspace_id).await?;
        for rec in &records {
            // Dedup by chunk_hash (sha256 of content)
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(rec.path.as_bytes());
            hasher.update(rec.revision.to_le_bytes());
            hasher.update(rec.updated_at.timestamp().to_le_bytes());
            let hash = crate::crypto::hex_encode(hasher.finalize());

            if seen.insert(hash.clone()) {
                entries.push(ManifestEntry {
                    chunk_hash: hash,
                    namespace: rec.workspace_id.clone(),
                    revision: rec.revision,
                    updated_at: rec.updated_at,
                    size_bytes: rec.content.len() as u64,
                    record_path: Some(rec.path.clone()),
                });
            }
        }
    }

    Ok(entries)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::memory::store::{MemoryRecord, MemoryStore};
    use std::sync::Arc;

    /// A minimal in-memory store for testing.
    pub(crate) struct TestStore {
        pub(crate) records: std::sync::Mutex<Vec<MemoryRecord>>,
    }

    #[async_trait::async_trait]
    impl MemoryStore for TestStore {
        fn backend(&self) -> crate::memory::store::MemoryBackend {
            crate::memory::store::MemoryBackend::Memory
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn health(&self) -> std::result::Result<String, anyhow::Error> {
            Ok("ok".into())
        }
        async fn put(
            &self,
            record: crate::memory::store::MemoryRecord,
        ) -> std::result::Result<(), anyhow::Error> {
            let mut records = self.records.lock().unwrap();
            // Replace existing record with same id, or append
            if let Some(pos) = records
                .iter()
                .position(|r| r.id == record.id || r.path == record.path)
            {
                records[pos] = record;
            } else {
                records.push(record);
            }
            Ok(())
        }
        async fn get(
            &self,
            _workspace_id: &str,
            id_or_path: &str,
        ) -> std::result::Result<Option<crate::memory::store::MemoryRecord>, anyhow::Error>
        {
            let records = self.records.lock().unwrap();
            Ok(records
                .iter()
                .find(|r| r.path == id_or_path || r.id == id_or_path)
                .cloned())
        }
        async fn update(
            &self,
            _record: crate::memory::store::MemoryRecord,
        ) -> std::result::Result<(), anyhow::Error> {
            Ok(())
        }
        async fn delete(
            &self,
            _workspace_id: &str,
            id_or_path: &str,
        ) -> std::result::Result<Option<crate::memory::store::MemoryRecord>, anyhow::Error>
        {
            let mut records = self.records.lock().unwrap();
            let pos = records
                .iter()
                .position(|r| r.path == id_or_path || r.id == id_or_path);
            match pos {
                Some(i) => Ok(Some(records.remove(i))),
                None => Ok(None),
            }
        }
        async fn list(
            &self,
            _workspace_id: &str,
        ) -> std::result::Result<Vec<MemoryRecord>, anyhow::Error> {
            let records = self.records.lock().unwrap();
            Ok(records.clone())
        }
        async fn list_workspaces(&self) -> std::result::Result<Vec<String>, anyhow::Error> {
            let records = self.records.lock().unwrap();
            let mut seen = std::collections::HashSet::new();
            let mut ids = Vec::new();
            for rec in records.iter() {
                if seen.insert(rec.workspace_id.clone()) {
                    ids.push(rec.workspace_id.clone());
                }
            }
            Ok(ids)
        }
        async fn search(
            &self,
            _workspace_id: &str,
            _query: &str,
            _filters: Option<&crate::memory::schema::MemoryQueryFilters>,
        ) -> std::result::Result<Vec<MemoryRecord>, anyhow::Error> {
            Ok(Vec::new())
        }
        async fn load_workspace_state(
            &self,
            _workspace_id: &str,
        ) -> std::result::Result<crate::memory::store::DurableWorkspaceState, anyhow::Error>
        {
            anyhow::bail!("not implemented")
        }
        async fn save_beliefs(
            &self,
            _workspace_id: &str,
            _beliefs: Vec<crate::domain::memory::belief::BeliefEdge>,
        ) -> std::result::Result<(), anyhow::Error> {
            Ok(())
        }
        async fn save_session_token(
            &self,
            _workspace_id: &str,
            _token: crate::memory::store::SessionTokenRecord,
        ) -> std::result::Result<(), anyhow::Error> {
            Ok(())
        }
        async fn is_session_token_valid(
            &self,
            _workspace_id: &str,
            _token: &str,
        ) -> std::result::Result<bool, anyhow::Error> {
            Ok(false)
        }
        async fn save_checkpoint(
            &self,
            _workspace_id: &str,
            _checkpoint: crate::checkpoint::Checkpoint,
        ) -> std::result::Result<(), anyhow::Error> {
            Ok(())
        }
        async fn load_checkpoint(
            &self,
            _workspace_id: &str,
            _task_id: &str,
            _name: &str,
        ) -> std::result::Result<Option<crate::checkpoint::Checkpoint>, anyhow::Error> {
            Ok(None)
        }
        async fn list_checkpoints(
            &self,
            _workspace_id: &str,
            _task_id: &str,
        ) -> std::result::Result<Vec<crate::checkpoint::Checkpoint>, anyhow::Error> {
            Ok(Vec::new())
        }
        async fn delete_checkpoint(
            &self,
            _workspace_id: &str,
            _task_id: &str,
            _name: &str,
        ) -> std::result::Result<(), anyhow::Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_build_manifest_empty_store() {
        let store = Arc::new(TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        });
        let manifest = build_manifest(&*store).await.unwrap();
        assert!(manifest.is_empty(), "empty store → empty manifest");
    }

    #[tokio::test]
    async fn test_build_manifest_with_records() {
        use chrono::Utc;

        let store = Arc::new(TestStore {
            records: std::sync::Mutex::new(vec![MemoryRecord {
                id: "rec1".into(),
                workspace_id: "default".into(),
                path: "/memories/test".into(),
                content: "hello world".into(),
                metadata: serde_json::Value::Null,
                embedding: vec![],
                created_at: Utc::now(),
                updated_at: Utc::now(),
                revision: 1,
                primary: false,
                parent_id: None,
                cluster_id: None,
                level: Default::default(),
                relation: None,
                clearance: Default::default(),
                revisions: vec![],
                encrypted_dek: None,
                content_iv: None,
                metadata_iv: None,
                score: 0.0,
                deleted_at: None,
                embedding_status: "ok".into(),
                embedding_attempts: 0,
            }]),
        });
        let manifest = build_manifest(&*store).await.unwrap();
        assert_eq!(manifest.len(), 1, "one record → one manifest entry");
        assert_eq!(manifest[0].namespace, "default");
    }
}
