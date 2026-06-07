//! Shared types and schemas for the HTTP API.
//!
//! This module defines the request and response structures used across different
//! HTTP modules, ensuring a consistent data format for searches, memory updates,
//! and retrieval operations.

use crate::agents::runtime::System3Mode;
use crate::consistency::regularization::CoherenceReport;
use crate::memory::schema::{
    ContextZone, EvidenceKind, MemoryKind, MemoryLevel, MemoryNamespace, MemoryProvenance,
    MemoryQueryFilters, RelationKind,
};
use crate::memory::store::HybridSearchMode;
use crate::retrieval::gating::LayerWeights;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub filters: Option<MemoryQueryFilters>,
    #[serde(default)]
    pub system3_mode: Option<System3Mode>,
}
fn default_limit() -> usize {
    10
}

#[derive(Debug, Deserialize)]
pub struct HybridSearchRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default, rename = "type")]
    pub search_type: Option<HybridSearchMode>,
    #[serde(default)]
    pub filters: Option<MemoryQueryFilters>,
    #[serde(default = "default_weight")]
    pub keyword_weight: f32,
    #[serde(default = "default_weight")]
    pub vector_weight: f32,
}
fn default_weight() -> f32 {
    0.5
}

#[derive(Debug, Deserialize)]
pub struct AddMemoryRequest {
    pub content: String,
    pub path: Option<String>,
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub kind: Option<MemoryKind>,
    #[serde(default)]
    pub evidence_kind: Option<EvidenceKind>,
    #[serde(default)]
    pub namespace: Option<MemoryNamespace>,
    #[serde(default)]
    pub provenance: Option<MemoryProvenance>,
    #[serde(default)]
    pub cluster_id: Option<String>,
    #[serde(default)]
    pub level: Option<MemoryLevel>,
    #[serde(default)]
    pub relation: Option<RelationKind>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub status: String,
    pub results: Vec<serde_json::Value>,
    pub query: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HybridSearchResponse {
    pub status: String,
    pub results: Vec<serde_json::Value>,
    pub query: String,
    pub mode: HybridSearchMode,
}

#[derive(Debug, Deserialize)]
pub struct MultiLayerRetrieveRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub layer_weights: Option<LayerWeights>,
    #[serde(default = "default_relevance_threshold")]
    pub relevance_threshold: f32,
    #[serde(default = "default_rrf_k")]
    pub rrf_k: u32,
    #[serde(default)]
    pub include_coherence: bool,
    #[serde(default)]
    pub active_zones: Option<Vec<ContextZone>>,
    #[serde(default = "default_recency_weight")]
    pub recency_weight: f32,
    #[serde(default = "default_half_life_hours")]
    pub half_life_hours: f32,
    #[serde(default = "default_true")]
    pub grounding_enabled: bool,
    #[serde(default = "default_grounding_threshold")]
    pub grounding_min_confidence: f32,
}
fn default_recency_weight() -> f32 {
    crate::retrieval::config::DEFAULT_RECENCY_WEIGHT
}
fn default_half_life_hours() -> f32 {
    crate::retrieval::config::DEFAULT_HALF_LIFE_HOURS
}
fn default_true() -> bool {
    true
}
fn default_grounding_threshold() -> f32 {
    0.5
}
fn default_relevance_threshold() -> f32 {
    0.5
}
fn default_rrf_k() -> u32 {
    crate::search::hybrid::configured_rrf_k()
}

#[derive(Debug, Serialize)]
pub struct MultiLayerRetrieveResponse {
    pub status: String,
    pub results: Vec<RetrievedMemory>,
    pub query: String,
    pub layers_used: LayerStatsJson,
    pub coherence_report: Option<CoherenceReport>,
}

#[derive(Debug, Deserialize)]
pub struct ExportPackRequest {
    pub topic: String,
    #[serde(default = "default_max_level")]
    pub max_level: usize,
}
fn default_max_level() -> usize {
    3
}

#[derive(Debug, Serialize)]
pub struct ExportPackResponse {
    pub status: String,
    pub xml: String,
    pub filename: String,
}

#[derive(Debug, Serialize)]
pub struct RetrievedMemory {
    pub id: String,
    pub content: String,
    pub score: f32,
    pub source_layer: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct LayerStatsJson {
    pub working_count: usize,
    pub episodic_count: usize,
    pub semantic_count: usize,
    pub total_results: usize,
}
