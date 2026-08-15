//! ProviderRouter for mini-experts endpoints.
//!
//! This router manages configured mini-experts and handles routing and invocation
//! for local, agy, or custom mini-expert endpoints.

use crate::agents::mini_experts::MiniExpertRegistry;
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

    /// Creates a ProviderRouter initialized from the default MiniExpertRegistry,
    /// merged with any provided configs.
    pub fn from_registry_and_configs(
        registry: &MiniExpertRegistry,
        additional_configs: Vec<MiniExpertConfig>,
    ) -> Self {
        let mut configs: Vec<MiniExpertConfig> =
            registry.list().into_iter().map(|e| e.to_config()).collect();

        for config in additional_configs {
            if !configs.iter().any(|c| c.name == config.name) {
                configs.push(config);
            }
        }

        Self::new(configs)
    }

    /// Adds or updates a mini-expert configuration.
    pub fn add_mini_expert(&mut self, config: MiniExpertConfig) {
        if let Some(pos) = self.mini_experts.iter().position(|e| e.name == config.name) {
            self.mini_experts[pos] = config;
        } else {
            self.mini_experts.push(config);
        }
    }

    /// Returns all configured mini-experts.
    pub fn mini_experts(&self) -> &[MiniExpertConfig] {
        &self.mini_experts
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
        if expert
            .api_key
            .as_deref()
            .unwrap_or("")
            .starts_with("mock-")
            || name.starts_with("mock-")
        {
            return Ok(format!(
                "Mock response from mini-expert '{}' (provider: {}) for prompt: '{}'",
                expert.name, expert.provider, prompt
            ));
        }

        // Real invocation endpoint resolution
        let endpoint = if expert.endpoint.contains("/v1")
            || expert.endpoint.contains("/api")
            || expert.endpoint.contains("/invoke")
        {
            expert.endpoint.clone()
        } else if expert.endpoint.ends_with('/') {
            format!("{}v1/chat/completions", expert.endpoint)
        } else {
            format!("{}/v1/chat/completions", expert.endpoint)
        };

        let mut req = self.client.post(&endpoint).json(&json!({
            "model": expert.name,
            "messages": [{"role": "user", "content": prompt}],
            "prompt": prompt
        }));

        if let Some(ref key) = expert.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req.send().await?;
        if response.status().is_success() {
            let body: serde_json::Value = response.json().await?;
            if let Some(text) = body.get("response").and_then(|v| v.as_str()) {
                Ok(text.to_string())
            } else if let Some(choices) = body.get("choices").and_then(|v| v.as_array()) {
                if let Some(first) = choices.first() {
                    if let Some(text) = first.get("text").and_then(|v| v.as_str()) {
                        return Ok(text.to_string());
                    }
                    if let Some(message) = first.get("message") {
                        if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
                            return Ok(content.to_string());
                        }
                    }
                }
                Ok(body.to_string())
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

    #[test]
    fn test_provider_router_from_registry() {
        let temp_dir = std::env::temp_dir().join(format!("xavier_test_router_{}", ulid::Ulid::new()));
        let db_file = temp_dir.join("mini_experts.json");

        let registry = MiniExpertRegistry::new(&db_file);
        let entry = crate::agents::mini_experts::MiniExpertEntry {
            name: "registry-expert".to_string(),
            segment: "test/segment".to_string(),
            language: "es".to_string(),
            clearance: 1,
            source_dataset: "ds-1".to_string(),
            model_gguf_path: "/path/model.gguf".to_string(),
            provider: "local".to_string(),
            endpoint: "http://localhost:11434/v1".to_string(),
            api_key: None,
        };
        registry.register(entry).unwrap();

        let additional = vec![MiniExpertConfig {
            name: "config-expert".to_string(),
            provider: "custom".to_string(),
            endpoint: "http://localhost:8080".to_string(),
            api_key: None,
        }];

        let router = ProviderRouter::from_registry_and_configs(&registry, additional);

        assert!(router.route("registry-expert").is_some());
        assert_eq!(router.route("registry-expert").unwrap().provider, "local");

        assert!(router.route("config-expert").is_some());
        assert_eq!(router.route("config-expert").unwrap().provider, "custom");

        let _ = std::fs::remove_dir_all(&temp_dir);
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
