//! Request and response types for the CLI HTTP API.
//!
//! This module defines the data structures used for communication between the
//! CLI and external clients, covering memory search, code analysis, and agent management.

use serde::Deserialize;
use std::collections::HashMap;
use xavier::memory::schema::{ContextZone, FederatedSearchRequest, MemoryQueryFilters};

#[derive(Debug, Deserialize)]
pub struct SearchPayload {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default, rename = "filters")]
    pub filters: Option<MemoryQueryFilters>,
    #[serde(default)]
    pub active_zones: Option<Vec<ContextZone>>,
}

#[derive(Debug, Deserialize)]
pub struct CodeScanPayload {
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CodeFindPayload {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub pattern: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CodeContextPayload {
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_token_budget")]
    pub budget_tokens: usize,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CodeGraphQueryPayload {
    pub query: String,
    #[serde(default = "default_graph_depth")]
    pub depth: usize,
    #[serde(default = "default_graph_limit")]
    pub limit: usize,
    #[serde(default)]
    pub edge_type: Option<String>,
    #[serde(default = "default_graph_budget")]
    pub budget_tokens: usize,
}

#[derive(Debug, Deserialize)]
pub struct CodeBlastRadiusPayload {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default = "default_graph_depth")]
    pub depth: usize,
    #[serde(default = "default_graph_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
pub struct AddPayload {
    pub content: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub cluster_id: Option<String>,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub relation: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteMemoryRequest {
    pub id: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMemoryRequest {
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct SecurityScanPayload {
    #[allow(dead_code)] // Usado solo via serde deserialization
    pub input: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MemoryQueryPayload {
    pub query: String,
    pub limit: Option<usize>,
    #[serde(default, rename = "filters")]
    pub _filters: Option<serde_json::Value>,
    #[serde(default)]
    pub federated: Option<FederatedSearchRequest>,
}

#[derive(Debug, Deserialize)]
pub struct EvictPayload {
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub threshold: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct TimelineQueryPayload {
    pub query: String,
    #[serde(default)]
    pub start_date: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub end_date: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
pub struct SessionCompactPayload {
    pub session_id: String,
    #[serde(default)]
    pub current_tokens: Option<usize>,
    #[serde(default = "default_compaction_threshold")]
    pub threshold_percent: f64,
}

#[derive(Debug, Deserialize)]
pub struct AgentRegisterPayload {
    pub agent_id: String,
    pub session_id: Option<String>,
    pub name: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub role: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AgentPushContextPayload {
    pub context: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ExportPayload {
    pub public: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ConsolidatePayload {
    #[serde(default)]
    pub nightly: bool,
}

#[derive(Debug, Deserialize)]
pub struct SwarmConfig {
    pub agents: Vec<SwarmAgentConfig>,
}

#[derive(Debug, Deserialize)]
pub struct SwarmAgentConfig {
    pub name: String,
    pub provider: String,
    pub model: Option<String>,
    pub skills: Option<Vec<String>>,
    pub context: Option<HashMap<String, String>>,
    pub task: String,
}

/// Default limit.
pub fn default_limit() -> usize {
    10
}

/// Default token budget.
pub fn default_token_budget() -> usize {
    800
}

/// Default graph depth.
pub fn default_graph_depth() -> usize {
    3
}

/// Default graph limit.
pub fn default_graph_limit() -> usize {
    50
}

/// Default graph budget.
pub fn default_graph_budget() -> usize {
    1200
}

/// Default min degree.
pub fn default_min_degree() -> u64 {
    3
}

/// Default min complexity.
pub fn default_min_complexity() -> f32 {
    5.0
}

/// Default compaction threshold.
pub fn default_compaction_threshold() -> f64 {
    80.0
}

#[derive(Debug, Deserialize)]
pub struct LendSecretPayload {
    pub secret_name: String,
    #[serde(default)]
    pub secret_value: Option<String>,
    pub agent_id: String,
    pub ttl_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct RevokeLeasePayload {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct UsageUpdatePayload {
    pub provider: String,
    pub percentage: f32,
}

#[derive(Debug, Deserialize)]
pub struct UsageCooldownPayload {
    pub provider: String,
    pub minutes: i64,
}

#[derive(Debug, Deserialize)]
pub struct UsageTrackPayload {
    pub provider: String,
    pub tokens: usize,
    pub status: u16,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub is_cache_hit: bool,
}

#[derive(Debug, Deserialize)]
pub struct ExportPackPayload {
    pub topic: String,
    #[serde(default = "default_max_level_val")]
    pub max_level: usize,
}

/// Default max level val.
pub fn default_max_level_val() -> usize {
    3
}

#[derive(Debug, Deserialize)]
pub struct CreateTokenPayload {
    pub name: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CodeGraphViewParams {
    #[serde(default = "default_graph_view_mode")]
    pub mode: String,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default = "default_graph_view_depth")]
    pub depth: usize,
    #[serde(default = "default_graph_view_limit")]
    pub limit: usize,
    #[serde(default)]
    pub edge_type: Option<String>,
    #[serde(default = "default_graph_view_include_file_nodes")]
    pub include_file_nodes: bool,
    #[serde(default = "default_graph_view_min_degree")]
    pub min_degree: u64,
}

/// Default graph view mode.
pub fn default_graph_view_mode() -> String {
    "overview".to_string()
}

/// Default graph view depth.
pub fn default_graph_view_depth() -> usize {
    3
}

/// Default graph view limit.
pub fn default_graph_view_limit() -> usize {
    150
}

/// Default graph view include file nodes.
pub fn default_graph_view_include_file_nodes() -> bool {
    false
}

/// Default graph view min degree.
pub fn default_graph_view_min_degree() -> u64 {
    3
}
