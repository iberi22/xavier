//! Gestalt Agent Inbound Adapter Plugin
//!
//! Provides the Gestalt agent protocol bridge connecting Gestalt agents
//! with Xavier's central event bus (`XavierEventBus`) and memory context core.

use crate::coordination::events::{XavierEvent, XavierEventBus};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info};

/// Health status of the Gestalt Inbound Adapter
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GestaltAdapterStatus {
    pub name: String,
    pub status: String,
    pub active_subscriptions: usize,
    pub processed_events_count: u64,
}

/// Structured events exchanged between Gestalt agents and Xavier bus
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GestaltEvent {
    /// Gestalt memory fragment saved
    MemoryFragmentSaved {
        agent_id: String,
        context: String,
        record_id: String,
    },
    /// Gestalt agent task state updated
    TaskStateUpdated {
        agent_id: String,
        task_id: String,
        status: String,
    },
    /// Generic Gestalt telemetry / ping
    TelemetryPing { agent_id: String, timestamp: i64 },
}

/// Gestalt Inbound Adapter instance
#[derive(Clone)]
pub struct GestaltAdapter {
    event_bus: XavierEventBus,
    processed_count: Arc<AtomicU64>,
}

impl GestaltAdapter {
    /// Create a new Gestalt adapter instance attached to Xavier's event bus
    pub fn new(event_bus: XavierEventBus) -> Self {
        Self {
            event_bus,
            processed_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Access the underlying XavierEventBus reference
    pub fn event_bus(&self) -> &XavierEventBus {
        &self.event_bus
    }

    /// Return current adapter health status
    pub fn health_status(&self) -> GestaltAdapterStatus {
        GestaltAdapterStatus {
            name: "gestalt_inbound_adapter".to_string(),
            status: "active".to_string(),
            active_subscriptions: 1,
            processed_events_count: self.processed_count.load(Ordering::Relaxed),
        }
    }

    /// Start the background bridge task consuming events from XavierEventBus
    pub fn start_event_bridge(&self) -> tokio::task::JoinHandle<()> {
        let mut rx = self.event_bus.subscribe();
        let counter = Arc::clone(&self.processed_count);

        tokio::spawn(async move {
            info!("Gestalt event bridge listening on XavierEventBus...");
            while let Ok(event) = rx.recv().await {
                counter.fetch_add(1, Ordering::Relaxed);
                match event {
                    XavierEvent::AgentTaskStarted { agent_id, task_id } => {
                        debug!(
                            "Gestalt bridge observed agent task start: {} - {}",
                            agent_id, task_id
                        );
                    }
                    XavierEvent::AgentTaskCompleted { agent_id, task_id } => {
                        debug!(
                            "Gestalt bridge observed agent task completion: {} - {}",
                            agent_id, task_id
                        );
                    }
                    XavierEvent::AgentTaskFailed {
                        agent_id,
                        task_id,
                        reason,
                    } => {
                        debug!(
                            "Gestalt bridge observed agent task failure: {} - {} ({})",
                            agent_id, task_id, reason
                        );
                    }
                    _ => {}
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gestalt_adapter_health_and_bridge() {
        let bus = XavierEventBus::new(10);
        let adapter = GestaltAdapter::new(bus.clone());

        assert_eq!(adapter.health_status().status, "active");
        assert_eq!(adapter.health_status().processed_events_count, 0);

        let handle = adapter.start_event_bridge();

        bus.publish(XavierEvent::AgentTaskStarted {
            agent_id: "gestalt-1".to_string(),
            task_id: "task-101".to_string(),
        })
        .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(adapter.health_status().processed_events_count, 1);
        handle.abort();
    }
}
