use tokio::sync::mpsc;
use ulid::Ulid;
use crate::coordination::message_bus::{MessageBus, MessageBusError, AgentMessage, MessageType};

impl MessageBus {
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

    /// Broadcast a message to all agents
    pub async fn broadcast(
        &self,
        sender: &str,
        content: serde_json::Value,
        topic: Option<&str>,
    ) -> Result<String, MessageBusError> {
        let mut message = AgentMessage::new(sender, MessageType::Task, content);
        message.topic = topic.map(String::from);
        let id = message.id.clone();

        self.publish(message).await?;
        Ok(id)
    }

    /// Send a request and wait for response with timeout
    pub async fn request(
        &self,
        sender: &str,
        receiver: &str,
        content: serde_json::Value,
        timeout_secs: u64,
    ) -> Result<AgentMessage, MessageBusError> {
        let correlation_id = Ulid::new().to_string();

        // Create response channel
        let (response_tx, mut response_rx) = mpsc::channel(1);

        {
            let mut channels = self.response_channels.write().await;
            channels.insert(correlation_id.clone(), response_tx);
        }

        // Send the request
        let message = AgentMessage::task(sender, content)
            .to(receiver)
            .with_correlation(&correlation_id)
            .reply_to_channel(&correlation_id);

        self.publish(message).await?;

        // Wait for response
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            response_rx.recv(),
        )
        .await
        .map_err(|_| MessageBusError::RequestTimeout(timeout_secs))?
        .ok_or(MessageBusError::ChannelClosed)?;

        // Cleanup
        {
            let mut channels = self.response_channels.write().await;
            channels.remove(&correlation_id);
        }

        Ok(result)
    }

    /// Handle incoming response for a request
    pub async fn handle_response(&self, message: AgentMessage) -> Result<(), MessageBusError> {
        if let Some(ref correlation_id) = message.correlation_id {
            let channels = self.response_channels.read().await;

            if let Some(tx) = channels.get(correlation_id) {
                let _ = tx.send(message).await;
            }
        }

        Ok(())
    }

    /// Receive a message for a specific agent (blocking)
    pub async fn receive(
        &self,
        agent_id: &str,
        timeout: Option<u64>,
    ) -> Result<Option<AgentMessage>, MessageBusError> {
        let queues = self.queues.read().await;
        if queues.contains_key(agent_id) {
            drop(queues);

            if let Some(secs) = timeout {
                tokio::time::sleep(std::time::Duration::from_secs(secs.min(1))).await;
            }

            return Ok(None);
        }

        Err(MessageBusError::AgentNotFound(agent_id.to_string()))
    }
}
