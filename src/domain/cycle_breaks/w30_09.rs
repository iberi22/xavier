//! Owned by issue [WAVE-30.09] (ADR-033 Wave 0).
//!
//! Material living here to cut the four module cycles that ran from `memory`
//! back into `session`, `notifications`, `retrieval` and `ports`. Every moved
//! item keeps a `pub use` re-export at its original path, so no caller and no
//! behaviour changes.
//!
//! Only the data definitions live here. The inherent and trait impls stay in the
//! module that owned them, because Rust resolves an impl anywhere in the crate:
//! `memory` can name the moved type and call its methods without importing the
//! module that carries the behaviour.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use xavier_core_logic::ScoredResult;

use crate::domain::memory::{MemoryQueryFilters, MemoryRecord};

// ─── Cycle cut `memory -> session` ─────────────────────────────────────────

/// Incoming session event from OpenClaw webhook
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventType {
    SessionStart,
    SessionEnd,
    Message,
    ToolCall,
    ToolResult,
    Error,
}

/// Raw session event payload from OpenClaw
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub session_id: String,
    pub event_type: SessionEventType,
    pub timestamp: DateTime<Utc>,
    pub content: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

// ─── Cycle cut `memory -> notifications` ───────────────────────────────────

/// Publish capability the memory sync needs from the notification island.
///
/// The contract lives in the domain and the island implements it, so
/// `memory::sync` no longer imports `crate::notifications`.
#[async_trait]
pub trait SyncNotifier: Send + Sync {
    /// Publish one notification on the memory island.
    async fn notify_memory(&self, title: &str, body: &str, severity: &str) -> anyhow::Result<()>;
}

/// Sync publisher; its implementation stays in the notification island.
pub struct MemoryIsland;

// ─── Cycle cut `memory -> retrieval` ───────────────────────────────────────

/// Layer weights for multi-layer retrieval fusion.
/// These control how much each memory layer contributes to final results.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayerWeights {
    /// Weight for working memory layer (default 0.3)
    pub working: f32,
    /// Weight for episodic memory layer (default 0.3)
    pub episodic: f32,
    /// Weight for semantic memory layer (default 0.4)
    pub semantic: f32,
}

/// Weights for graph traversal signals
///
/// These weights are used by the `NavigationPolicy` to score and rank
/// potential transitions (edges) during graph-based memory traversal.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TraversalWeights {
    /// Importance of text/semantic match between query and target node.
    pub semantic_similarity: f32,
    /// Importance of the edge's inherent confidence score (from indexer).
    pub confidence: f32,
    /// Importance of the edge weight/strength signal.
    pub edge_weight: f32,
    /// Importance of temporal recency (newer edges favored).
    pub recency: f32,
    /// Bonus for transitions that cross memory layers (e.g., Working to Semantic).
    pub cross_layer: f32,
    /// Bonus/penalty for crossing directory boundaries in the codebase.
    pub cross_dir: f32,
    /// Bonus for navigating towards high-degree "hub" nodes.
    pub peripheral_hub: f32,
}

/// Learned navigation policy for retrieval and traversal
///
/// The `NavigationPolicy` manages the weights used to gate different memory
/// layers (Working, Episodic, Semantic) and the weights used to navigate
/// the belief graph. It can be updated online via reinforcement learning
/// (e.g., HORMER's GRPO implementation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationPolicy {
    /// Current base weights for each memory layer.
    pub layer_weights: LayerWeights,
    /// Weights for graph traversal signals.
    pub traversal_weights: TraversalWeights,
    /// Learning rate for weight updates (default 0.01).
    pub learning_rate: f32,
    /// Number of updates applied to this policy.
    pub update_count: u64,
    /// Last reward received (0.0-1.0) during an update.
    pub last_reward: f32,
    /// Historical average reward (running average).
    pub avg_reward: f32,
}

/// Result from a multi-layer search (for context pack export)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayeredSearchResult {
    pub topic: String,
    pub timestamp: String,
    pub level_0_working: Vec<ScoredResult>,
    pub level_1_entity_graph: Vec<ScoredResult>,
    pub level_2_semantic: Vec<ScoredResult>,
    pub level_3_episodic: Vec<ScoredResult>,
}

// ─── Cycle cut `memory -> ports` ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_doc: bool,
    pub id: Option<String>,
}

/// Inbound port for memory operations.
///
/// The contract lives in the domain so `memory` can depend on the port without
/// depending on the `ports` module, which itself depends on the memory domain
/// types (`MemoryQueryFilters`, `MemoryRecord`).
#[async_trait]
pub trait MemoryQueryPort: Send + Sync {
    async fn search(
        &self,
        query: &str,
        limit: usize,
        filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>>;
    async fn expand_depth(
        &self,
        results: &[MemoryRecord],
        depth: usize,
        filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>>;
    async fn add(&self, record: MemoryRecord) -> anyhow::Result<String>;
    async fn update(&self, id: &str, record: MemoryRecord) -> anyhow::Result<MemoryRecord>;
    async fn delete(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>>;
    async fn get(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>>;
    async fn list(&self, workspace_id: &str, limit: usize) -> anyhow::Result<Vec<MemoryRecord>>;
    async fn export(&self, public_only: bool) -> anyhow::Result<Vec<MemoryRecord>>;
    async fn ls(&self, path: &str) -> anyhow::Result<Vec<NavEntry>>;
}

/// Outbound port for schema initialization.
pub trait SchemaInitializer: Send + Sync {
    fn init_schema(&self) -> anyhow::Result<()>;
}
