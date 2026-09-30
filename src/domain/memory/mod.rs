//! Domain models for memory operations.
//!
//! These types define the core domain contract for memory querying, storage,
//! and time metrics. Inbound ports reference these domain types rather than
//! infrastructure types, preserving hexagonal architecture DIP.

use serde::{Deserialize, Serialize};

/// Re-export the canonical MemoryQueryFilters from memory::schema for now,
/// as the schema is the authoritative definition.
pub mod belief;

pub use crate::memory::schema::MemoryQueryFilters;
pub use crate::memory::store::MemoryRecord;
pub use belief::{BeliefEdge, BeliefNode};

pub mod graph;

/// Core TimeMetric domain value — NOT a DTO.
/// Used by the TimeMetrics inbound port to decouple from HTTP DTOs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeMetric {
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
