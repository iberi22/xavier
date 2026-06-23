//! Event coordination and dispatch
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::tasks::models::Task;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum XavierEvent {
    TaskCompleted { task: Task },
    TaskFailed { task: Task, reason: String },
    ToolCalled { name: String, args: Value, session_id: String },
    ToolSucceeded { name: String, result: Value, session_id: String },
    ToolFailed { name: String, error: String, session_id: String },
    ErrorOccurred { message: String, details: Value },
}

#[derive(Clone)]
pub struct XavierEventBus {
    sender: broadcast::Sender<XavierEvent>,
}

impl XavierEventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<XavierEvent> {
        self.sender.subscribe()
    }

    pub fn publish(
        &self,
        event: XavierEvent,
    ) -> Result<usize, Box<broadcast::error::SendError<XavierEvent>>> {
        self.sender.send(event).map_err(Box::new)
    }
}

pub struct WebhookDispatcher {
    client: Client,
    endpoints: Vec<String>,
}

impl WebhookDispatcher {
    pub fn new(endpoints: Vec<String>) -> Self {
        Self {
            client: Client::new(),
            endpoints,
        }
    }

    pub fn start(&self, mut receiver: broadcast::Receiver<XavierEvent>) {
        let client = self.client.clone();
        let endpoints = self.endpoints.clone();

        tokio::spawn(async move {
            info!("WebhookDispatcher started listening for events.");
            while let Ok(event) = receiver.recv().await {
                if let XavierEvent::TaskCompleted { task } = &event {
                    info!("Task {} completed! Dispatching webhooks...", task.id);
                    for endpoint in &endpoints {
                        let payload = serde_json::json!({
                            "event_type": "TaskCompleted",
                            "task": task,
                        });

                        let res = client.post(endpoint).json(&payload).send().await;

                        match res {
                            Ok(response) => {
                                if !response.status().is_success() {
                                    error!(
                                        "Webhook to {} failed with status {}",
                                        endpoint,
                                        response.status()
                                    );
                                }
                            }
                            Err(e) => error!("Failed to send webhook to {}: {}", endpoint, e),
                        }
                    }
                }
            }
        });
    }
}
