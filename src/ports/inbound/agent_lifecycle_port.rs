// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Inbound port for agent lifecycle management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::domain::agent::{AgentEntry, AgentMetadata};
use async_trait::async_trait;

/// Port for agent lifecycle management.
///
/// Abstracts agent registration, heartbeats, and active agent queries.
#[async_trait]
pub trait AgentLifecyclePort: Send + Sync {
    /// Register a new agent with metadata.
    async fn register(&self, agent_id: String, session_id: String, metadata: AgentMetadata)
        -> bool;

    /// Unregister an agent by ID.
    async fn unregister(&self, agent_id: &str) -> bool;

    /// Update heartbeat for an agent.
    async fn heartbeat(&self, agent_id: &str) -> bool;

    /// Get all active agents (heartbeat < 5 minutes old).
    async fn get_active_agents(&self) -> Vec<AgentEntry>;

    /// Get a specific agent entry.
    async fn get(&self, agent_id: &str) -> Option<AgentEntry>;

    /// Hook called when a task starts.
    async fn on_task_start(&self, agent_id: &str, task_id: &str);

    /// Hook called when a task completes.
    async fn on_task_complete(
        &self,
        agent_id: &str,
        task_id: &str,
        result: &Result<crate::agents::runtime::AgentResponse, String>,
    );
}
