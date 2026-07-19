//! Coordination Core - Integrates events with system components.
//!
//! Listens for agent task events and manages secret leases automatically.

use crate::coordination::events::{XavierEvent, XavierEventBus};
use crate::coordination::secrets::KeyLendingEngine;
use std::sync::Arc;
use tracing::info;

/// Coordinator that bridges events to the KeyLendingEngine for lease management.
pub struct CoordinationCore {
    event_bus: XavierEventBus,
    secrets_engine: Arc<KeyLendingEngine>,
}

impl CoordinationCore {
    pub fn new(event_bus: XavierEventBus, secrets_engine: Arc<KeyLendingEngine>) -> Self {
        Self {
            event_bus,
            secrets_engine,
        }
    }

    /// Start the coordination loop to listen for events.
    pub fn start(&self) {
        let mut receiver = self.event_bus.subscribe();
        let secrets_engine = self.secrets_engine.clone();

        tokio::spawn(async move {
            info!("CoordinationCore started listening for events.");
            while let Ok(event) = receiver.recv().await {
                match event {
                    XavierEvent::AgentTaskStarted { agent_id, task_id } => {
                        info!(
                            "Handling AgentTaskStarted: agent={}, task={}",
                            agent_id, task_id
                        );
                        // Future improvement: Automatically lend default keys if configured
                    }
                    XavierEvent::AgentTaskCompleted { agent_id, task_id } => {
                        info!(
                            "Handling AgentTaskCompleted: agent={}, task={}. Revoking leases.",
                            agent_id, task_id
                        );
                        let count = secrets_engine
                            .revoke_for_agent(&agent_id, "Task Completed")
                            .await;
                        info!("Revoked {} leases for agent {}", count, agent_id);
                    }
                    XavierEvent::AgentTaskFailed {
                        agent_id,
                        task_id,
                        reason,
                    } => {
                        info!("Handling AgentTaskFailed: agent={}, task={}, reason={}. Revoking leases.", agent_id, task_id, reason);
                        let count = secrets_engine
                            .revoke_for_agent(&agent_id, &format!("Task Failed: {}", reason))
                            .await;
                        if count > 0 {
                            info!("Revoked {} leases for agent {}", count, agent_id);
                        }
                    }
                    _ => {} // Other events handled elsewhere
                }
            }
        });
    }
}
