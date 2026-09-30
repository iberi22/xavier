//! Runtime contracts for ADR-033 Wave 0.

use super::w30_06::ConversationMessage;
use super::w30_12::Checkpoint;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[async_trait::async_trait]
pub trait ContextOrchestratorPort: Send + Sync {
    async fn session_start(&self, session_id: &str, prompt: &str) -> usize;
    async fn precompact(&self, session_id: &str, prompt: &str) -> usize;
}

#[async_trait::async_trait]
pub trait TgdRuntimePort: Send + Sync {
    fn confidence_threshold(&self) -> f32;
    async fn generate_rules(
        &self,
        history: &[ConversationMessage],
        context: &[RetrievedDocument],
    ) -> Result<String>;
}

#[async_trait::async_trait]
pub trait CheckpointPort: Send + Sync {
    fn as_any(&self) -> &dyn std::any::Any;
    async fn save(&self, checkpoint: Checkpoint) -> Result<()>;
    async fn load(&self, task_id: String, name: String) -> Result<Option<Checkpoint>>;
    async fn list(&self, task_id: String) -> Result<Vec<Checkpoint>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedDocument {
    pub id: String,
    pub path: String,
    pub content: String,
    pub relevance_score: f32,
    pub token_count: usize,
    pub metadata: serde_json::Value,
}

/// Log queries required by the agent gap analyzer.
#[async_trait::async_trait]
pub trait GapLogPort: Send + Sync {
    async fn entry_counts(&self) -> anyhow::Result<(u64, u64)>;
    async fn error_modules(&self, minutes: u32, threshold: u32) -> anyhow::Result<Vec<String>>;
    async fn latency_metadata(&self, limit: u32) -> anyhow::Result<Vec<Option<serde_json::Value>>>;
}
