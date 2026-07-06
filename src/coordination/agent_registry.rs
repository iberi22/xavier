//! Agent Registry - Track and manage active agents with heartbeats.
//!
//! Provides a simple in-memory registry for agents to:
//! - Register with a session ID
//! - Send heartbeats to indicate liveness
//! - Query active agents (heartbeat < 5 minutes)
//! - Store/retrieve agent context in memory

use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::ports::inbound::AgentLifecyclePort;

pub use crate::domain::agent::{AgentEntry, AgentMetadata};

const HEARTBEAT_TIMEOUT_SECS: i64 = 300; // 5 minutes

/// Agent registry for tracking active agents
pub struct SimpleAgentRegistry {
    agents: RwLock<HashMap<String, AgentEntry>>,
    secrets_engine: Option<Arc<crate::coordination::KeyLendingEngine>>,
    event_bus: Option<crate::coordination::events::XavierEventBus>,
}

impl Default for SimpleAgentRegistry {
    fn default() -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
            secrets_engine: None,
            event_bus: None,
        }
    }
}

impl SimpleAgentRegistry {
    /// Create a new registry
    pub fn new(event_bus: Option<crate::coordination::events::XavierEventBus>) -> Arc<Self> {
        Arc::new(Self {
            agents: RwLock::new(HashMap::new()),
            event_bus,
        })
    }

    /// Create a new registry with engines
    pub fn new_with_engines(
        secrets_engine: Option<Arc<crate::coordination::KeyLendingEngine>>,
        event_bus: Option<crate::coordination::events::XavierEventBus>,
    ) -> Arc<Self> {
        Arc::new(Self {
            agents: RwLock::new(HashMap::new()),
            secrets_engine,
            event_bus,
        })
    }

    /// Register a new agent
    pub async fn register(
        &self,
        agent_id: String,
        session_id: String,
        metadata: AgentMetadata,
    ) -> bool {
        let now = Utc::now();
        let mut agents = self.agents.write().await;

        let entry = AgentEntry {
            agent_id: agent_id.clone(),
            session_id,
            last_heartbeat: now,
            metadata,
        };

        agents.insert(agent_id, entry);
        true
    }

    /// Unregister an agent
    pub async fn unregister(&self, agent_id: &str) -> bool {
        let mut agents = self.agents.write().await;
        agents.remove(agent_id).is_some()
    }

    /// Update heartbeat for an agent
    pub async fn heartbeat(&self, agent_id: &str) -> bool {
        let mut agents = self.agents.write().await;
        if let Some(entry) = agents.get_mut(agent_id) {
            entry.last_heartbeat = Utc::now();
            true
        } else {
            false
        }
    }

    /// Get all active agents (heartbeat < 5 minutes old)
    pub async fn get_active_agents(&self) -> Vec<AgentEntry> {
        let now = Utc::now();
        let agents = self.agents.read().await;

        agents
            .values()
            .filter(|entry| {
                let age = now.signed_duration_since(entry.last_heartbeat);
                age.num_seconds() < HEARTBEAT_TIMEOUT_SECS
            })
            .cloned()
            .collect()
    }

    /// Get a specific agent
    pub async fn get(&self, agent_id: &str) -> Option<AgentEntry> {
        let agents = self.agents.read().await;
        agents.get(agent_id).cloned()
    }

    /// List all registered agent IDs
    pub async fn list_ids(&self) -> Vec<String> {
        let agents = self.agents.read().await;
        agents.keys().cloned().collect()
    }
}

#[async_trait]
impl AgentLifecyclePort for SimpleAgentRegistry {
    async fn register(
        &self,
        agent_id: String,
        session_id: String,
        metadata: AgentMetadata,
    ) -> bool {
        SimpleAgentRegistry::register(self, agent_id, session_id, metadata).await
    }

    async fn unregister(&self, agent_id: &str) -> bool {
        SimpleAgentRegistry::unregister(self, agent_id).await
    }

    async fn heartbeat(&self, agent_id: &str) -> bool {
        SimpleAgentRegistry::heartbeat(self, agent_id).await
    }

    async fn get_active_agents(&self) -> Vec<AgentEntry> {
        SimpleAgentRegistry::get_active_agents(self).await
    }

    async fn get(&self, agent_id: &str) -> Option<AgentEntry> {
        SimpleAgentRegistry::get(self, agent_id).await
    }

    async fn on_task_start(&self, agent_id: &str, task_id: &str) {
        tracing::info!("Task {} started for agent {}", task_id, agent_id);

        // Notify event bus
        if let Some(bus) = &self.event_bus {
            let _ = bus.publish(crate::coordination::events::XavierEvent::AgentTaskStarted {
                agent_id: agent_id.to_string(),
                task_id: task_id.to_string(),
            }).await;
        }

        // Renew leases for the agent if they exist
        if let Some(engine) = &self.secrets_engine {
            engine.renew_for_agent(agent_id, 3600).await;
        }
    }

    async fn on_task_complete(
        &self,
        agent_id: &str,
        task_id: &str,
        result: &Result<crate::agents::runtime::AgentResponse, String>,
    ) {
        match result {
            Ok(_) => {
                tracing::info!("Task {} completed for agent {}", task_id, agent_id);
                if let Some(bus) = &self.event_bus {
                    let _ = bus.publish(crate::coordination::events::XavierEvent::AgentTaskCompleted {
                        agent_id: agent_id.to_string(),
                    }).await;
                }
            }
            Err(e) => {
                tracing::error!("Task {} failed for agent {}: {}", task_id, agent_id, e);
                if let Some(bus) = &self.event_bus {
                    let _ = bus.publish(crate::coordination::events::XavierEvent::AgentTaskFailed {
                        agent_id: agent_id.to_string(),
                        reason: e.to_string(),
                    }).await;
                }
            }
        }

        // Revoke leases for the agent
        if let Some(engine) = &self.secrets_engine {
            engine.revoke_for_agent(agent_id, "Task Ended").await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_heartbeat() {
        let registry = SimpleAgentRegistry::new(None);

        // Register an agent
        let meta = AgentMetadata {
            name: Some("test-agent".to_string()),
            capabilities: vec!["coding".to_string()],
            role: Some("worker".to_string()),
            endpoint: None,
        };

        let result = registry
            .register("agent-1".to_string(), "session-abc".to_string(), meta)
            .await;
        assert!(result);

        // Get active agents
        let active = registry.get_active_agents().await;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].agent_id, "agent-1");
        assert_eq!(active[0].session_id, "session-abc");

        // Heartbeat
        let result = registry.heartbeat("agent-1").await;
        assert!(result);

        // Unregister
        let result = registry.unregister("agent-1").await;
        assert!(result);

        let active = registry.get_active_agents().await;
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn test_get_active_agents_filters_stale() {
        let registry = SimpleAgentRegistry::new(None);

        let meta = AgentMetadata::default();
        registry
            .register("agent-1".to_string(), "s1".to_string(), meta.clone())
            .await;

        // Add another agent
        registry
            .register("agent-2".to_string(), "s2".to_string(), meta.clone())
            .await;

        let active = registry.get_active_agents().await;
        assert_eq!(active.len(), 2);

        // Manually expire one agent (modify its heartbeat in the map)
        {
            let mut agents = registry.agents.write().await;
            if let Some(entry) = agents.get_mut("agent-1") {
                entry.last_heartbeat = Utc::now() - chrono::Duration::seconds(400);
            }
        }

        let active = registry.get_active_agents().await;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].agent_id, "agent-2");
    }
}
