// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Inbound port for session management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::session::types::SessionEvent;
use async_trait::async_trait;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SessionEventResult {
    pub status: String,
    pub session_id: String,
    pub memory_id: Option<String>,
    pub mapped: bool,
}

#[async_trait]
pub trait SessionPort: Send + Sync {
    async fn handle_event(&self, event: SessionEvent) -> bool;
    async fn handle_and_index_event(
        &self,
        event: SessionEvent,
    ) -> anyhow::Result<SessionEventResult>;
}
