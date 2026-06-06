use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::coordination::message_bus::{
    AgentInfo, AgentMessage, BusMetrics, DLQEntry, HeartbeatConfig, MessageBusError,
};

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

    /// Subscribe an agent to a topic
    pub async fn subscribe(&self, agent_id: &str, topic: &str) -> Result<(), MessageBusError> {
        // Verify agent exists
        {
            let agents = self.agents.read().await;
            if !agents.contains_key(agent_id) {
                return Err(MessageBusError::AgentNotFound(agent_id.to_string()));
            }
        }

        let mut topics = self.topics.write().await;
        topics
            .entry(topic.to_string())
            .or_default()
            .insert(agent_id.to_string());

        {
            let mut metrics = self.metrics.write().await;
            metrics.topic_subscriptions =
                topics.iter().map(|(k, v)| (k.clone(), v.len())).collect();
        }

        tracing::debug!("Agent {} subscribed to topic {}", agent_id, topic);

        Ok(())
    }

    /// Unsubscribe an agent from a topic
    pub async fn unsubscribe(&self, agent_id: &str, topic: &str) -> Result<(), MessageBusError> {
        let mut topics = self.topics.write().await;

        if let Some(subscribers) = topics.get_mut(topic) {
            subscribers.remove(agent_id);
        }

        {
            let mut metrics = self.metrics.write().await;
            metrics.topic_subscriptions =
                topics.iter().map(|(k, v)| (k.clone(), v.len())).collect();
        }

        tracing::debug!("Agent {} unsubscribed from topic {}", agent_id, topic);

        Ok(())
    }

    /// Publish a message to the bus
    pub async fn publish(&self, message: AgentMessage) -> Result<usize, MessageBusError> {
        {
            let mut metrics = self.metrics.write().await;
            metrics.messages_sent += 1;
        }

        let receivers = self.determine_receivers(&message).await;

        if receivers.is_empty() {
            tracing::warn!("Message {} has no receivers", message.id);
            return Ok(0);
        }

        let mut sent_count = 0;

        for receiver_id in &receivers {
            if let Some(tx) = self.queues.read().await.get(receiver_id) {
                if tx.send(message.clone()).await.is_err() {
                    tracing::warn!("Failed to send message to agent {}", receiver_id);
                }
                sent_count += 1;
            }
        }

        // Also broadcast to topic subscribers
        if let Some(ref topic) = message.topic {
            let _ = self.broadcast_tx.send(message.clone());

            // Get topic subscribers
            let topics = self.topics.read().await;
            if let Some(subscribers) = topics.get(topic) {
                for subscriber_id in subscribers {
                    if !receivers.contains(subscriber_id) {
                        if let Some(tx) = self.queues.read().await.get(subscriber_id) {
                            if tx.send(message.clone()).await.is_ok() {
                                sent_count += 1;
                            }
                        }
                    }
                }
            }
        }

        tracing::debug!(
            "Message {} published to {} receivers",
            message.id,
            sent_count
        );

        Ok(sent_count)
    }

    /// Send a direct message to an agent
    pub async fn send_direct(
        &self,
        sender: &str,
        receiver: &str,
        content: serde_json::Value,
    ) -> Result<String, MessageBusError> {
        let message = AgentMessage::task(sender, content).to(receiver);
        let id = message.id.clone();
        self.publish(message).await?;
        Ok(id)
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
