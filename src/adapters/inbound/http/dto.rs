//! Data Transfer Objects for HTTP API communication
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::domain::memory::TimeMetric;
use crate::domain::pattern::{PatternCategory, PatternVerification};
use crate::domain::security::ThreatLevel;
use serde::{Deserialize, Serialize};

// ─── Time Metrics ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeMetricDto {
    pub metric_type: String,
    pub agent_id: String,
    pub task_id: Option<String>,
    pub started_at: String,
    pub completed_at: String,
    pub duration_ms: u64,
    pub status: String,
    pub error_message: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub tokens_used: Option<u64>,
    pub task_category: Option<String>,
    pub metadata: serde_json::Value,
}

// The conversions live here, next to the DTO they produce, so the domain type
// does not have to import the HTTP adapter to declare them.
impl From<TimeMetric> for TimeMetricDto {
    fn from(m: TimeMetric) -> Self {
        Self {
            metric_type: m.metric_type,
            agent_id: m.agent_id,
            task_id: m.task_id,
            started_at: m.started_at,
            completed_at: m.completed_at,
            duration_ms: m.duration_ms,
            status: m.status,
            error_message: m.error_message,
            provider: m.provider,
            model: m.model,
            tokens_used: m.tokens_used,
            task_category: m.task_category,
            metadata: m.metadata,
        }
    }
}

impl From<TimeMetricDto> for TimeMetric {
    fn from(dto: TimeMetricDto) -> Self {
        Self {
            metric_type: dto.metric_type,
            agent_id: dto.agent_id,
            task_id: dto.task_id,
            started_at: dto.started_at,
            completed_at: dto.completed_at,
            duration_ms: dto.duration_ms,
            status: dto.status,
            error_message: dto.error_message,
            provider: dto.provider,
            model: dto.model,
            tokens_used: dto.tokens_used,
            task_category: dto.task_category,
            metadata: dto.metadata,
        }
    }
}

// ─── Pattern Protocol ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct PatternDiscoverRequest {
    pub pattern: String,
    pub category: PatternCategory,
    pub project: String,
    pub confidence: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PatternResponse {
    pub id: String,
    pub category: PatternCategory,
    pub pattern: String,
    pub confidence: f32,
    pub verification: PatternVerification,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecurityScanRequest {
    pub target: String,
    pub level: Option<ThreatLevel>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecurityScanResponse {
    pub id: String,
    pub threats_count: usize,
    pub scan_duration_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryAddRequest {
    pub content: String,
    pub namespace: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemorySearchRequest {
    pub query: String,
    pub namespace: Option<String>,
    pub limit: Option<usize>,
}
