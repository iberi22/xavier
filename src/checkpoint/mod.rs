//! System state checkpointing module
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
pub mod session;
pub mod state;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

use crate::domain::cycle_breaks::w30_12::MemoryConsumerStore;

pub use session::{SessionCheckpoint, SessionCheckpointInput, MAX_SESSION_CHECKPOINT_BYTES};

pub use crate::domain::cycle_breaks::w30_12::Checkpoint;

pub struct CheckpointManager {
    checkpoints: RwLock<HashMap<String, Checkpoint>>,
    workspace_id: Option<String>,
    store: Option<Arc<dyn MemoryConsumerStore>>,
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckpointManager {
    /// New.
    pub fn new() -> Self {
        Self {
            checkpoints: RwLock::new(HashMap::new()),
            workspace_id: None,
            store: None,
        }
    }

    /// With store.
    pub fn with_store<S: MemoryConsumerStore + ?Sized + 'static>(
        workspace_id: impl Into<String>,
        store: Arc<S>,
    ) -> Self {
        Self {
            checkpoints: RwLock::new(HashMap::new()),
            workspace_id: Some(workspace_id.into()),
            store: Some(store.into_consumer_store()),
        }
    }

    fn key(task_id: &str, name: &str) -> String {
        format!("{task_id}::{name}")
    }

    /// Save.
    pub async fn save(&self, checkpoint: Checkpoint) -> Result<()> {
        self.checkpoints.write().await.insert(
            Self::key(&checkpoint.task_id, &checkpoint.name),
            checkpoint.clone(),
        );
        if let (Some(workspace_id), Some(store)) = (&self.workspace_id, &self.store) {
            store
                .save_checkpoint_record(workspace_id, checkpoint)
                .await?;
        }
        Ok(())
    }

    /// Load.
    pub async fn load(&self, task_id: String, name: String) -> Result<Option<Checkpoint>> {
        if let Some(checkpoint) = self
            .checkpoints
            .read()
            .await
            .get(&Self::key(&task_id, &name))
            .cloned()
        {
            return Ok(Some(checkpoint));
        }

        if let (Some(workspace_id), Some(store)) = (&self.workspace_id, &self.store) {
            let checkpoint = store
                .load_checkpoint_record(workspace_id, &task_id, &name)
                .await?;
            if let Some(checkpoint) = checkpoint.clone() {
                self.checkpoints
                    .write()
                    .await
                    .insert(Self::key(&task_id, &name), checkpoint);
            }
            return Ok(checkpoint);
        }

        Ok(None)
    }

    /// List.
    pub async fn list(&self, task_id: String) -> Result<Vec<Checkpoint>> {
        if let (Some(workspace_id), Some(store)) = (&self.workspace_id, &self.store) {
            let checkpoints = store
                .list_checkpoint_records(workspace_id, &task_id)
                .await?;
            let mut cache = self.checkpoints.write().await;
            for checkpoint in &checkpoints {
                cache.insert(
                    Self::key(&checkpoint.task_id, &checkpoint.name),
                    checkpoint.clone(),
                );
            }
            return Ok(checkpoints);
        }

        Ok(self
            .checkpoints
            .read()
            .await
            .values()
            .filter(|checkpoint| checkpoint.task_id == task_id)
            .cloned()
            .collect())
    }

    /// Delete.
    pub async fn delete(&self, task_id: String, name: String) -> Result<()> {
        self.checkpoints
            .write()
            .await
            .remove(&Self::key(&task_id, &name));
        if let (Some(workspace_id), Some(store)) = (&self.workspace_id, &self.store) {
            store
                .delete_checkpoint_record(workspace_id, &task_id, &name)
                .await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::domain::cycle_breaks::w30_03::CheckpointPort for Arc<CheckpointManager> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn save(&self, checkpoint: Checkpoint) -> Result<()> {
        self.as_ref().save(checkpoint).await
    }

    async fn load(&self, task_id: String, name: String) -> Result<Option<Checkpoint>> {
        self.as_ref().load(task_id, name).await
    }

    async fn list(&self, task_id: String) -> Result<Vec<Checkpoint>> {
        self.as_ref().list(task_id).await
    }
}

impl crate::agents::runtime::AgentRuntime {
    /// With checkpoint manager.
    pub fn with_checkpoint_manager(mut self, manager: Arc<CheckpointManager>) -> Self {
        self.checkpoint_manager = Some(Box::new(manager));
        self
    }

    /// Checkpoint manager.
    pub fn checkpoint_manager(&self) -> Option<&Arc<CheckpointManager>> {
        self.checkpoint_manager.as_ref()?.as_any().downcast_ref()
    }
}

impl crate::agents::runtime::RuntimeBuilder {
    /// With checkpoint manager.
    pub fn with_checkpoint_manager(mut self, manager: Arc<CheckpointManager>) -> Self {
        self.checkpoint_manager = Some(Box::new(manager));
        self
    }
}
