use crate::coordination::message_bus::MessageBus;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Message Bus metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BusMetrics {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub messages_failed: u64,
    pub dlq_size: usize,
    pub registered_agents: usize,
    pub topic_subscriptions: HashMap<String, usize>,
    pub queue_sizes: HashMap<String, usize>,
    pub stale_agents: usize,
    pub heartbeats_received: u64,
}

impl MessageBus {
    /// Get bus metrics
    pub async fn get_metrics(&self) -> BusMetrics {
        let mut metrics = self.metrics.write().await;

        // Update queue sizes
        let queues = self.queues.read().await;
        metrics.queue_sizes = queues
            .iter()
            .map(|(k, v)| (k.clone(), v.capacity()))
            .collect();

        metrics.clone()
    }
}
