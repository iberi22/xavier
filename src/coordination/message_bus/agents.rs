use chrono::Utc;
use crate::coordination::message_bus::{MessageBus, MessageBusError, AgentInfo, AgentStatus};

impl MessageBus {
    /// Update agent heartbeat
    pub async fn heartbeat(&self, agent_id: &str) -> Result<(), MessageBusError> {
        let mut agents = self.agents.write().await;

        if let Some(agent) = agents.get_mut(agent_id) {
            agent.last_heartbeat = Utc::now();
            agent.status = AgentStatus::Active;

            // Update metrics
            drop(agents);
            let mut metrics = self.metrics.write().await;
            metrics.heartbeats_received += 1;
            metrics.stale_agents = 0; // Reset, will be recalculated

            return Ok(());
        }

        Err(MessageBusError::AgentNotFound(agent_id.to_string()))
    }

    /// Get agents that have stale heartbeats ( haven't sent heartbeat within timeout )
    pub async fn get_stale_agents(&self) -> Vec<AgentInfo> {
        let agents = self.agents.read().await;
        let timeout = chrono::Duration::seconds(self.heartbeat_config.timeout_secs as i64);
        let now = Utc::now();

        agents
            .values()
            .filter(|a| now.signed_duration_since(a.last_heartbeat) > timeout)
            .cloned()
            .collect()
    }

    /// Mark stale agents as offline
    pub async fn mark_stale_offline(&self) -> usize {
        let stale = self.get_stale_agents().await;
        let mut count = 0;

        let mut agents = self.agents.write().await;
        for agent in &stale {
            if let Some(a) = agents.get_mut(&agent.id) {
                a.status = AgentStatus::Offline;
                count += 1;
            }
        }

        // Update metrics
        drop(agents);
        let mut metrics = self.metrics.write().await;
        metrics.stale_agents = count;

        if count > 0 {
            tracing::warn!("Marked {} agents as offline due to stale heartbeat", count);
        }

        count
    }

    /// Check if an agent's heartbeat is stale
    pub async fn is_heartbeat_stale(&self, agent_id: &str) -> bool {
        let agents = self.agents.read().await;

        if let Some(agent) = agents.get(agent_id) {
            let timeout = chrono::Duration::seconds(self.heartbeat_config.timeout_secs as i64);
            return Utc::now().signed_duration_since(agent.last_heartbeat) > timeout;
        }

        false
    }

    /// Get agent info
    pub async fn get_agent(&self, agent_id: &str) -> Option<AgentInfo> {
        let agents = self.agents.read().await;
        agents.get(agent_id).cloned()
    }

    /// List all registered agents
    pub async fn list_agents(&self) -> Vec<AgentInfo> {
        let agents = self.agents.read().await;
        agents.values().cloned().collect()
    }
}
