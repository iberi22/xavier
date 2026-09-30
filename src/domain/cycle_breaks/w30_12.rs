//! Shared records and consumer contracts for ADR-033 Wave 0.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use xavier_core_logic::{ClearanceLevel, MemoryLevel, RelationKind};

use crate::utils::crypto::hex_encode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRevision {
    pub revision: u64,
    pub recorded_at: DateTime<Utc>,
    pub path: String,
    pub content: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub workspace_id: String,
    pub path: String,
    pub content: String,
    pub metadata: serde_json::Value,
    pub embedding: Vec<f32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revision: u64,
    pub primary: bool,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub cluster_id: Option<String>,
    #[serde(default)]
    pub level: MemoryLevel,
    #[serde(default)]
    pub relation: Option<RelationKind>,
    #[serde(default)]
    pub clearance: ClearanceLevel,
    #[serde(default)]
    pub revisions: Vec<MemoryRevision>,
    #[serde(default)]
    pub encrypted_dek: Option<Vec<u8>>,
    #[serde(default)]
    pub content_iv: Option<Vec<u8>>,
    #[serde(default)]
    pub metadata_iv: Option<Vec<u8>>,
    #[serde(default)]
    pub score: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(default = "default_embedding_status")]
    pub embedding_status: String,
    #[serde(default)]
    pub embedding_attempts: u32,
}

fn default_embedding_status() -> String {
    "pending".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub task_id: String,
    pub name: String,
    pub data: serde_json::Value,
}

impl Checkpoint {
    /// New.
    pub fn new(task_id: String, name: String, data: serde_json::Value) -> Self {
        Self {
            task_id,
            name,
            data,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VecSqliteStoreConfig {
    pub path: PathBuf,
    pub embedding_dimensions: usize,
}

/// Operations required by checkpointing, embedding, and command failure consumers.
#[async_trait]
pub trait MemoryConsumerStore: Send + Sync {
    fn into_consumer_store(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn MemoryConsumerStore>
    where
        Self: 'static;

    async fn put_record(&self, record: MemoryRecord) -> Result<()>;
    async fn update_record(&self, record: MemoryRecord) -> Result<()>;
    async fn list_records(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>>;
    async fn save_checkpoint_record(
        &self,
        workspace_id: &str,
        checkpoint: Checkpoint,
    ) -> Result<()>;
    async fn load_checkpoint_record(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>>;
    async fn list_checkpoint_records(
        &self,
        workspace_id: &str,
        task_id: &str,
    ) -> Result<Vec<Checkpoint>>;
    async fn delete_checkpoint_record(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<()>;
}

/// Factory contract preserving the backend's concrete store type.
#[async_trait]
pub trait VecStoreBackend {
    type Store: Clone + Send + Sync;

    async fn open(path: PathBuf) -> Result<Self::Store>;
    fn project_id_for_path(path: &Path) -> String;
}

pub struct DefaultVecStoreBackend;
pub type VecStore = <DefaultVecStoreBackend as VecStoreBackend>::Store;

/// Stable key.
pub fn stable_key(kind: &str, parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    digest.update(kind.as_bytes());
    for part in parts {
        digest.update([0u8]);
        digest.update(part.as_bytes());
    }
    hex_encode(&digest.finalize())
}
