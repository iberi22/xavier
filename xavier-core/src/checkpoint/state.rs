//! Checkpoint state stub for xavier-core
//!
//! TODO: Re-integrate or align with main crate's state checkpointing when ready.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointState {
    pub session_id: String,
    pub task_queue: Vec<String>,
    pub tools_state: HashMap<String, serde_json::Value>,
    pub checkpoint_timestamp: DateTime<Utc>,
}

impl CheckpointState {
    pub fn new(
        session_id: impl Into<String>,
        task_queue: Vec<String>,
        tools_state: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            task_queue,
            tools_state,
            checkpoint_timestamp: Utc::now(),
        }
    }
}

pub async fn save_checkpoint(_state: &CheckpointState) -> Result<PathBuf> {
    Err(anyhow!("state::save_checkpoint is not implemented in xavier-core"))
}

pub async fn load_latest_checkpoint(_session_id: &str) -> Result<CheckpointState> {
    Err(anyhow!("state::load_latest_checkpoint is not implemented in xavier-core"))
}

pub async fn is_session_restorable(_session_id: &str) -> Result<bool> {
    Ok(false)
}
