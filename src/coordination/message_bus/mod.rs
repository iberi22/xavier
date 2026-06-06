//! Message Bus for Agent Coordination
//!
//! Provides async message passing between agents using tokio channels.
//! Supports pub/sub, direct messaging, request/response, and broadcast.
//!
//! Architecture:
//! - Per-agent queues for receiving messages
//! - Topic-based pub/sub subscriptions
//! - Request/response with timeout support
//! - Dead Letter Queue for failed messages
//!
//! Based on: RESEARCH_agent_coordination.md

pub mod types;
pub mod errors;
pub mod metrics;
pub mod dlq;
pub mod routing;
pub mod dispatch;
pub mod agents;

#[cfg(test)]
mod tests;

pub use types::*;
pub use errors::*;
pub use metrics::*;
pub use dlq::*;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use chrono::Utc;
use tokio::sync::{broadcast, mpsc, RwLock};

/// The main Message Bus for agent coordination
pub struct MessageBus {
    /// Per-agent message receivers
    pub(crate) queues: RwLock<HashMap<String, mpsc::Sender<AgentMessage>>>,

    /// Topic subscriptions (topic -> set of agent IDs)
    pub(crate) topics: RwLock<HashMap<String, HashSet<String>>>,

    /// Broadcast channel for topic messages
    pub(crate) broadcast_tx: broadcast::Sender<AgentMessage>,

    /// Response channels for request/response
    pub(crate) response_channels: RwLock<HashMap<String, mpsc::Sender<AgentMessage>>>,

    /// Dead Letter Queue with metadata
    pub(crate) dlq: RwLock<Vec<DLQEntry>>,

    /// Metrics
    pub(crate) metrics: RwLock<BusMetrics>,

    /// Registered agents
    pub(crate) agents: RwLock<HashMap<String, AgentInfo>>,

    /// Heartbeat configuration
    pub(crate) heartbeat_config: HeartbeatConfig,
}

impl MessageBus {
    /// Create a new MessageBus with default config
    pub fn new() -> Arc<Self> {
        Self::with_config(HeartbeatConfig::default())
    }

    /// Create a new MessageBus with custom heartbeat config
    pub fn with_config(config: HeartbeatConfig) -> Arc<Self> {
        let (broadcast_tx, _) = broadcast::channel(1000);

        Arc::new(Self {
            queues: RwLock::new(HashMap::new()),
            topics: RwLock::new(HashMap::new()),
            broadcast_tx,
            response_channels: RwLock::new(HashMap::new()),
            dlq: RwLock::new(Vec::new()),
            metrics: RwLock::new(BusMetrics::default()),
            agents: RwLock::new(HashMap::new()),
            heartbeat_config: config,
        })
    }

    /// Register an agent with the message bus
    pub async fn register_agent(
        &self,
        agent_id: &str,
        name: &str,
        capabilities: Vec<String>,
    ) -> Result<mpsc::Receiver<AgentMessage>, MessageBusError> {
        let (tx, rx) = mpsc::channel(100);

        {
            let mut queues = self.queues.write().await;
            if queues.contains_key(agent_id) {
                return Err(MessageBusError::AgentAlreadyRegistered(
                    agent_id.to_string(),
                ));
            }
            queues.insert(agent_id.to_string(), tx);
        }

        {
            let mut agents = self.agents.write().await;
            agents.insert(
                agent_id.to_string(),
                AgentInfo {
                    id: agent_id.to_string(),
                    name: name.to_string(),
                    capabilities,
                    registered_at: Utc::now(),
                    last_heartbeat: Utc::now(),
                    status: AgentStatus::Active,
                },
            );
        }

        {
            let mut metrics = self.metrics.write().await;
            metrics.registered_agents = self.agents.read().await.len();
        }

        tracing::info!("Agent {} registered with message bus", agent_id);

        Ok(rx)
    }

    /// Unregister an agent
    pub async fn unregister_agent(&self, agent_id: &str) -> Result<(), MessageBusError> {
        {
            let mut queues = self.queues.write().await;
            queues.remove(agent_id);
        }

        {
            let mut agents = self.agents.write().await;
            if let Some(agent) = agents.get_mut(agent_id) {
                agent.status = AgentStatus::Offline;
            }
            agents.remove(agent_id);
        }

        {
            let mut topics = self.topics.write().await;
            for subscribers in topics.values_mut() {
                subscribers.remove(agent_id);
            }
        }

        {
            let mut metrics = self.metrics.write().await;
            metrics.registered_agents = self.agents.read().await.len();
        }

        tracing::info!("Agent {} unregistered from message bus", agent_id);

        Ok(())
    }

    /// Shutdown the message bus
    pub async fn shutdown(&self) {
        // Clear all queues
        {
            let mut queues = self.queues.write().await;
            queues.clear();
        }

        // Clear agents
        {
            let mut agents = self.agents.write().await;
            agents.clear();
        }

        tracing::info!("Message bus shutdown complete");
    }
}
