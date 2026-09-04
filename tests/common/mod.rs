//! Shared integration test utilities for memory sync, manifests, and record management.

use std::sync::Arc;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use xavier::crypto::hex_encode;
use xavier::memory::cloud_sync::{CloudMemorySync, CloudSyncConfig, SYNC_BATCH_SIZE};
use xavier::memory::store::{MemoryRecord, MemoryStore};
use xavier::memory::sync::manifest::build_manifest;
use xavier::memory::sync::Manifest;

/// Compute a deterministic SHA-256 hash of a `Manifest`.
///
/// Manifest entries are sorted deterministically by namespace, record path,
/// and chunk hash before hashing to guarantee that identical store contents
/// yield identical manifest hashes regardless of iteration order.
pub fn compute_manifest_hash(manifest: &Manifest) -> String {
    let mut sorted = manifest.clone();
    sorted.sort_by(|a, b| {
        let ns_cmp = a.namespace.cmp(&b.namespace);
        if ns_cmp != std::cmp::Ordering::Equal {
            return ns_cmp;
        }
        let path_a = a.record_path.as_deref().unwrap_or_default();
        let path_b = b.record_path.as_deref().unwrap_or_default();
        let path_cmp = path_a.cmp(path_b);
        if path_cmp != std::cmp::Ordering::Equal {
            return path_cmp;
        }
        a.chunk_hash.cmp(&b.chunk_hash)
    });

    let mut hasher = Sha256::new();
    for entry in &sorted {
        hasher.update(entry.chunk_hash.as_bytes());
        hasher.update(entry.namespace.as_bytes());
        hasher.update(entry.revision.to_le_bytes());
        hasher.update(entry.updated_at.timestamp().to_le_bytes());
        hasher.update(entry.size_bytes.to_le_bytes());
        if let Some(ref path) = entry.record_path {
            hasher.update(path.as_bytes());
        }
    }
    hex_encode(hasher.finalize())
}

/// Helper to build a store's manifest and compute its deterministic SHA-256 hash.
pub async fn compute_store_manifest_hash(store: &dyn MemoryStore) -> anyhow::Result<String> {
    let manifest = build_manifest(store).await?;
    Ok(compute_manifest_hash(&manifest))
}

/// Helper to construct a test `MemoryRecord` with metadata and timestamp details.
pub fn make_record(
    id: &str,
    workspace_id: &str,
    path: &str,
    content: &str,
    updated_at: DateTime<Utc>,
    revision: u64,
    node_id: &str,
    deleted_at: Option<DateTime<Utc>>,
) -> MemoryRecord {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "node_id".to_string(),
        serde_json::Value::String(node_id.to_string()),
    );

    MemoryRecord {
        id: id.to_string(),
        workspace_id: workspace_id.to_string(),
        path: path.to_string(),
        content: content.to_string(),
        metadata: serde_json::Value::Object(metadata),
        embedding: Vec::new(),
        created_at: updated_at,
        updated_at,
        revision,
        deleted_at,
        ..Default::default()
    }
}

/// Helper to initialize a `CloudMemorySync` backed by a remote/cloud store.
pub async fn create_cloud_sync(
    cloud_store: Arc<dyn MemoryStore>,
    node_id: &str,
    batch_size: Option<usize>,
) -> (CloudMemorySync, TempDir) {
    let tmp_dir = TempDir::new().expect("create temp dir for cloud sync test");
    let config = CloudSyncConfig {
        data_dir: tmp_dir.path().to_string_lossy().to_string(),
        node_id: Some(node_id.to_string()),
        batch_size: batch_size.unwrap_or(SYNC_BATCH_SIZE),
        ..Default::default()
    };
    let sync = CloudMemorySync::new(cloud_store, config)
        .await
        .expect("instantiate CloudMemorySync");
    (sync, tmp_dir)
}
