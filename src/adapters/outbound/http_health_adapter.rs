//! HTTP health check adapter for external services
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub status: String,
    pub lag_ms: u64,
    pub active_agents: usize,
}

/// HTTP adapter that calls the /xavier/health endpoint on the remote Xavier instance.
pub struct HttpHealthAdapter {
    base_url: String,
    client: reqwest::Client,
}

impl HttpHealthAdapter {
    pub fn new(base_url: String, client: reqwest::Client) -> Self {
        Self { base_url, client }
    }

    pub async fn check_health(&self) -> anyhow::Result<HealthStatus> {
        let url = format!("{}/health", self.base_url);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return anyhow::Ok(HealthStatus {
                status: "degraded".to_string(),
                lag_ms: 0,
                active_agents: 0,
            });
        }

        let body: serde_json::Value = response.json().await?;

        let status = body
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("ok")
            .to_string();

        let lag_ms = body.get("lag_ms").and_then(|v| v.as_u64()).unwrap_or(0);

        let active_agents = body
            .get("active_agents")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        Ok(HealthStatus {
            status,
            lag_ms,
            active_agents,
        })
    }
}
