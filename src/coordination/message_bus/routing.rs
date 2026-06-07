use crate::coordination::message_bus::{AgentMessage, MessageBus};
use std::collections::HashMap;

impl MessageBus {
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
