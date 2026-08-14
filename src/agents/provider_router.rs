//! ProviderRouter for mini-experts endpoints.
//!
//! This router manages configured mini-experts and handles routing and invocation
//! for local, agy, or custom mini-expert endpoints.

use crate::settings::types::MiniExpertConfig;
use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::json;

/// Router for directing calls to mini-expert endpoints.
pub struct ProviderRouter {
    mini_experts: Vec<MiniExpertConfig>,
    client: Client,
}

impl ProviderRouter {
    /// Creates a new ProviderRouter with the given configuration.
    pub fn new(mini_experts: Vec<MiniExpertConfig>) -> Self {
        Self {
            mini_experts,
            client: Client::new(),
        }
    }

    /// Finds a mini-expert config by name.
    pub fn route(&self, name: &str) -> Option<&MiniExpertConfig> {
        self.mini_experts.iter().find(|e| e.name == name)
    }

    /// Invokes the mini-expert endpoint by sending a POST request to its configured url.
    pub async fn invoke(&self, name: &str, prompt: &str) -> Result<String> {
        let expert = self
            .route(name)
            .ok_or_else(|| anyhow!("Mini-expert '{}' not found", name))?;

        // If api_key starts with "mock-" or if name starts with "mock-", return mock response.
        if expert.api_key.as_deref().unwrap_or("").starts_with("mock-") || name.starts_with("mock-")
        {
            return Ok(format!(
                "Mock response from mini-expert '{}' (provider: {}) for prompt: '{}'",
                expert.name, expert.provider, prompt
            ));
        }

        // Real invocation
        let mut req = self
            .client
            .post(&expert.endpoint)
            .json(&json!({ "prompt": prompt }));

        if let Some(ref key) = expert.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req.send().await?;
        if response.status().is_success() {
            let body: serde_json::Value = response.json().await?;
            if let Some(text) = body.get("response").and_then(|v| v.as_str()) {
                Ok(text.to_string())
            } else if let Some(choices) = body.get("choices").and_then(|v| v.as_array()) {
                if let Some(text) = choices
                    .first()
                    .and_then(|c| c.get("text"))
                    .and_then(|v| v.as_str())
                {
                    Ok(text.to_string())
                } else if let Some(message) = choices.first().and_then(|c| c.get("message")) {
                    if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
                        return Ok(content.to_string());
                    }
                    Ok(body.to_string())
                } else {
                    Ok(body.to_string())
                }
            } else {
                Ok(body.to_string())
            }
        } else {
            Err(anyhow!(
                "Failed to invoke mini-expert: HTTP {}",
                response.status()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mini_expert_routing() {
        let configs = vec![
            MiniExpertConfig {
                name: "agy-expert".to_string(),
                provider: "agy".to_string(),
                endpoint: "https://api.agy.ai/v1/experts/google".to_string(),
                api_key: Some("mock-key".to_string()),
            },
            MiniExpertConfig {
                name: "local-expert".to_string(),
                provider: "local".to_string(),
                endpoint: "http://localhost:11434/v1".to_string(),
                api_key: None,
            },
        ];

        let router = ProviderRouter::new(configs);

        let routed = router.route("agy-expert");
        assert!(routed.is_some());
        assert_eq!(routed.unwrap().provider, "agy");

        let routed_local = router.route("local-expert");
        assert!(routed_local.is_some());
        assert_eq!(routed_local.unwrap().provider, "local");
    }

    #[test]
    fn test_mini_expert_unconfigured_routing() {
        let router = ProviderRouter::new(vec![]);
        assert!(router.route("non-existent").is_none());
    }

    #[tokio::test]
    async fn test_mini_expert_mock_invocation() {
        let configs = vec![MiniExpertConfig {
            name: "mock-expert".to_string(),
            provider: "custom".to_string(),
            endpoint: "http://localhost:12345/api".to_string(),
            api_key: Some("mock-key".to_string()),
        }];

        let router = ProviderRouter::new(configs);
        let resp = router.invoke("mock-expert", "hello!").await.unwrap();
        assert!(resp.contains("Mock response"));
        assert!(resp.contains("mock-expert"));
        assert!(resp.contains("custom"));
        assert!(resp.contains("hello!"));
    }

    #[tokio::test]
    async fn test_mini_expert_real_http_invocation() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/invoke")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"response": "Real response from custom expert!"}"#)
            .create_async()
            .await;

        let configs = vec![MiniExpertConfig {
            name: "real-expert".to_string(),
            provider: "custom".to_string(),
            endpoint: format!("{}/invoke", server.url()),
            api_key: Some("real-key".to_string()),
        }];

        let router = ProviderRouter::new(configs);
        let resp = router.invoke("real-expert", "ping").await.unwrap();
        assert_eq!(resp, "Real response from custom expert!");
    }
}
