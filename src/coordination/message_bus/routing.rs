use std::collections::HashMap;
use crate::coordination::message_bus::{MessageBus, MessageBusError, AgentMessage};

impl MessageBus {
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

    /// Get topic subscriptions
    pub async fn get_topics(&self) -> HashMap<String, Vec<String>> {
        let topics = self.topics.read().await;
        topics
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().cloned().collect()))
            .collect()
    }

    /// Determine message receivers based on receiver/topic
    pub(crate) async fn determine_receivers(&self, message: &AgentMessage) -> Vec<String> {
        // Direct message
        if let Some(ref receiver) = message.receiver {
            return vec![receiver.clone()];
        }

        // Topic-based
        if let Some(ref topic) = message.topic {
            let topics = self.topics.read().await;
            if let Some(subscribers) = topics.get(topic) {
                return subscribers.iter().cloned().collect();
            }
        }

        // Broadcast to all
        let queues = self.queues.read().await;
        queues.keys().cloned().collect()
    }
}
