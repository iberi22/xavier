//! Owned by issue [WAVE-30.07] (ADR-033 Wave 0).

use serde::{Deserialize, Serialize};

/// Internal event broadcasted across the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeEvent {
    pub workspace_id: String,
    pub event_id: String,
    pub agent_id: String,
    pub project_id: Option<String>,
    pub event_type: String,
    pub timestamp: String,
    pub payload: serde_json::Value,
}
