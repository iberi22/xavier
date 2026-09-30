//! Pure memory contracts for ADR-033 Wave 0.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use xavier_core_logic::{ClearanceLevel, ContextZone, MemoryLevel, RelationKind};

pub use super::w30_12::{MemoryRecord, MemoryRevision};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Episodic,
    Semantic,
    Procedural,
    Belief,
    Org,
    Workspace,
    User,
    Agent,
    Session,
    Event,
    Fact,
    Decision,
    Repo,
    Branch,
    File,
    Symbol,
    Url,
    Task,
    Contact,
    Meeting,
    ContentProject,
    VideoAsset,
    Document,
    Harness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    SourceTurn,
    SessionSummary,
    TemporalEvent,
    FactAtom,
    EntityState,
    SummaryFact,
    Observation,
    UserPrompt,
    ExecutionTrace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalScope {
    Global,
    Zone,
    Detailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederatedSearchRequest {
    #[serde(default)]
    pub local_dbs: Vec<String>,
    #[serde(default)]
    pub peer_nodes: Vec<String>,
    #[serde(default)]
    pub propagate_to_mesh: bool,
    #[serde(default = "default_max_hops")]
    pub max_hops: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryQueryFilters {
    pub kinds: Option<Vec<MemoryKind>>,
    pub evidence_kinds: Option<Vec<EvidenceKind>>,
    pub org_id: Option<String>,
    pub workspace_id: Option<String>,
    pub user_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub scope: Option<String>,
    pub retrieval_scope: Option<RetrievalScope>,
    pub source_app: Option<String>,
    pub source_type: Option<String>,
    pub repo_url: Option<String>,
    pub file_path: Option<String>,
    pub symbol: Option<String>,
    pub url: Option<String>,
    pub message_id: Option<String>,
    pub topic_key: Option<String>,
    pub observed_after: Option<String>,
    pub observed_before: Option<String>,
    pub recorded_after: Option<String>,
    pub recorded_before: Option<String>,
    pub cluster_ids: Option<Vec<String>>,
    pub levels: Option<Vec<MemoryLevel>>,
    pub zones: Option<Vec<ContextZone>>,
    pub clearances: Option<Vec<ClearanceLevel>>,
    #[serde(default)]
    pub path_prefix: Option<String>,
    /// When `false` (the default), general search excludes telemetry/noise
    /// namespaces (`activity/*`, `gestalt/thinking/*`, and records tagged
    /// as automatic activity/insight events). Callers that explicitly want
    /// that telemetry back (e.g. an activity dashboard) set this to `true`.
    #[serde(default)]
    pub include_activity: Option<bool>,
    #[serde(default)]
    pub federated: Option<FederatedSearchRequest>,
}

fn default_max_hops() -> u8 {
    1
}
