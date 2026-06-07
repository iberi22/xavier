//! Dead letter queue for failed message delivery.
//!
//! Captures undeliverable messages with metadata about delivery attempts
//! and failures, enabling debugging and reprocessing of stuck messages.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use crate::coordination::message_bus::{MessageBus, AgentMessage};

/// Dead Letter Queue entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DLQEntry {
    pub message: AgentMessage,
    pub failed_at: chrono::DateTime<Utc>,
    pub failure_reason: String,
    pub retry_count: u32,
}

impl DLQEntry {
    pub fn new(message: AgentMessage, reason: &str) -> Self {
        Self {
            message,
            failed_at: Utc::now(),
            failure_reason: reason.to_string(),
            retry_count: 0,
        }
    }

    pub fn with_retry(mut self, count: u32) -> Self {
        self.retry_count = count;
        self
    }
}

impl MessageBus {
    /// Send a message to the Dead Letter Queue
    pub async fn send_to_dlq(
        &self,
        message: AgentMessage,
        reason: &str,
    ) -> Result<(), crate::coordination::message_bus::MessageBusError> {
        let entry = DLQEntry::new(message, reason);
        let mut dlq = self.dlq.write().await;
        dlq.push(entry);

        let mut metrics = self.metrics.write().await;
        metrics.messages_failed += 1;
        metrics.dlq_size = dlq.len();

        tracing::error!("Message sent to DLQ: {}", reason);

        Ok(())
    }

    /// Send a failed message to DLQ with retry count
    pub async fn send_to_dlq_with_retry(
        &self,
        message: AgentMessage,
        reason: &str,
        retry_count: u32,
    ) -> Result<(), crate::coordination::message_bus::MessageBusError> {
        let entry = DLQEntry::new(message, reason).with_retry(retry_count);
        let mut dlq = self.dlq.write().await;
        dlq.push(entry);

        let mut metrics = self.metrics.write().await;
        metrics.messages_failed += 1;
        metrics.dlq_size = dlq.len();

        tracing::error!(
            "Message sent to DLQ after {} retries: {}",
            retry_count,
            reason
        );

        Ok(())
    }

    /// Get messages from Dead Letter Queue
    pub async fn get_dlq(&self) -> Vec<DLQEntry> {
        let dlq = self.dlq.read().await;
        dlq.clone()
    }

    /// Get DLQ size
    pub async fn get_dlq_size(&self) -> usize {
        let dlq = self.dlq.read().await;
        dlq.len()
    }

    /// Clear Dead Letter Queue
    pub async fn clear_dlq(&self) -> usize {
        let mut dlq = self.dlq.write().await;
        let size = dlq.len();
        dlq.clear();

        let mut metrics = self.metrics.write().await;
        metrics.dlq_size = 0;

        size
    }

    /// Remove and return a specific message from DLQ for reprocessing
    pub async fn reprocess_dlq_message(&self, message_id: &str) -> Option<AgentMessage> {
        let mut dlq = self.dlq.write().await;

        if let Some(pos) = dlq.iter().position(|e| e.message.id == message_id) {
            let entry = dlq.remove(pos);

            let mut metrics = self.metrics.write().await;
            metrics.dlq_size = dlq.len();

            tracing::info!("Reprocessing DLQ message: {}", message_id);
            return Some(entry.message);
        }

        None
    }
}
