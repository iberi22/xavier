//! Lifecycle hooks for agents.
//!
//! Provides functions to emit events when agent tasks start, complete, or fail.

use crate::coordination::events::{XavierEvent, XavierEventBus};
use tracing::{info, error};

/// Publish an event indicating that an agent has started a task.
pub fn publish_task_start(
    bus: &XavierEventBus,
    agent_id: String,
    task_id: String,
) {
    info!("Agent {} starting task {}", agent_id, task_id);
    if let Err(e) = bus.publish(XavierEvent::AgentTaskStarted {
        agent_id,
        task_id,
    }) {
        error!("Failed to publish AgentTaskStarted: {}", e);
    }
}

/// Publish an event indicating that an agent has completed a task.
pub fn publish_task_complete(
    bus: &XavierEventBus,
    agent_id: String,
    task_id: String,
) {
    info!("Agent {} completed task {}", agent_id, task_id);
    if let Err(e) = bus.publish(XavierEvent::AgentTaskCompleted {
        agent_id,
        task_id,
    }) {
        error!("Failed to publish AgentTaskCompleted: {}", e);
    }
}

/// Publish an event indicating that an agent has failed a task.
pub fn publish_task_failure(
    bus: &XavierEventBus,
    agent_id: String,
    task_id: String,
    reason: String,
) {
    info!("Agent {} failed task {}: {}", agent_id, task_id, reason);
    if let Err(e) = bus.publish(XavierEvent::AgentTaskFailed {
        agent_id,
        task_id,
        reason,
    }) {
        error!("Failed to publish AgentTaskFailed: {}", e);
    }
}
