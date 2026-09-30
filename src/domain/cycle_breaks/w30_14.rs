//! Owned by issue [WAVE-30.14] (ADR-033 Wave 0).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;

/// Priority
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    #[default]
    Medium,
    High,
    Urgent,
}

/// Task status
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    #[default]
    Backlog,
    InProgress,
    Done,
    Failed,
}

impl std::str::FromStr for TaskStatus {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "backlog" => Ok(TaskStatus::Backlog),
            "in_progress" | "inprogress" | "progress" | "working" => Ok(TaskStatus::InProgress),
            "done" | "completed" | "complete" => Ok(TaskStatus::Done),
            "failed" | "error" => Ok(TaskStatus::Failed),
            _ => Ok(TaskStatus::Backlog),
        }
    }
}

impl TaskStatus {
    /// To planka list.
    pub fn to_planka_list(&self) -> &'static str {
        match self {
            TaskStatus::Backlog => "Backlog",
            TaskStatus::InProgress => "In Progress",
            TaskStatus::Done => "Done",
            TaskStatus::Failed => "Failed",
        }
    }
}

/// Core Task structure - backend agnostic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique identifier
    pub id: String,

    /// Task title
    pub title: String,

    /// Detailed description
    pub description: String,

    /// Project this task belongs to
    pub project: String,

    /// Current status
    pub status: TaskStatus,

    /// Priority level
    pub priority: Priority,

    /// Tags/labels
    pub labels: Vec<String>,

    /// Assignee (user ID or name)
    pub assignee: Option<String>,

    /// Who created this task
    pub created_by: String,

    /// Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,

    /// External sync (Planka)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planka_card_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planka_list_id: Option<String>,
}

/// Response final del agente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub session_id: String,
    pub query: String,
    pub response: String,
    pub confidence: f32,
    pub system_timings: SystemTimings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemTimings {
    pub system1_ms: u64,
    pub system2_ms: u64,
    pub system3_ms: u64,
    pub total_ms: u64,
}
