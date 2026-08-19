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
    /// New.
    pub fn new(base_url: String, client: reqwest::Client) -> Self {
        Self { base_url, client }
    }

    /// Check health.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_check_health_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/health")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "ok", "lag_ms": 42, "active_agents": 3}"#)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let adapter = HttpHealthAdapter::new(server.url(), client);

        let res = adapter.check_health().await.unwrap();
        assert_eq!(res.status, "ok");
        assert_eq!(res.lag_ms, 42);
        assert_eq!(res.active_agents, 3);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_check_health_non_success_status() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/health")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let adapter = HttpHealthAdapter::new(server.url(), client);

        let res = adapter.check_health().await.unwrap();
        assert_eq!(res.status, "degraded");
        assert_eq!(res.lag_ms, 0);
        assert_eq!(res.active_agents, 0);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_check_health_missing_json_fields_defaults() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/health")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{}")
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let adapter = HttpHealthAdapter::new(server.url(), client);

        let res = adapter.check_health().await.unwrap();
        assert_eq!(res.status, "ok");
        assert_eq!(res.lag_ms, 0);
        assert_eq!(res.active_agents, 0);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_check_health_network_error() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(100))
            .build()
            .unwrap();
        // Point to invalid non-routable IP/port
        let adapter = HttpHealthAdapter::new("http://127.0.0.1:59999".to_string(), client);

        let res = adapter.check_health().await;
        assert!(res.is_err());
    }
}
