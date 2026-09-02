//! Health check port — port-based health monitoring dispatch.

use crate::memory::MemoryStore;
use crate::ports::inbound::health_port::HealthStatus;
use std::sync::Arc;

pub struct HealthCheckPort {
    store: Option<Arc<dyn MemoryStore>>,
}

impl HealthCheckPort {
    /// New.
    pub fn new() -> Self {
        Self { store: None }
    }

    /// Create health check port with a memory store.
    pub fn with_store(store: Arc<dyn MemoryStore>) -> Self {
        Self { store: Some(store) }
    }

    /// Check health by dispatching to the underlying memory store.
    pub async fn check(&self) -> anyhow::Result<HealthStatus> {
        let status_str = if let Some(ref store) = self.store {
            store.health().await?
        } else {
            "uninitialized".to_string()
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(HealthStatus {
            status: status_str,
            lag_ms: 0,
            save_ok_rate: 1.0,
            match_score: 1.0,
            active_agents: 0,
            timestamp_ms: now,
            alerts: Vec::new(),
        })
    }
}

impl Default for HealthCheckPort {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check_port_uninitialized() {
        let port = HealthCheckPort::new();
        let health = port.check().await.unwrap();
        assert_eq!(health.status, "uninitialized");
    }
}
