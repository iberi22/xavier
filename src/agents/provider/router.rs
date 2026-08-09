//! Provider router for hot-switching LLM providers.
//!
//! This module implements the logic for switching providers at runtime,
//! managing fallback chains, and automatic strategy-based provider selection.

use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

/// Supported provider types for the router.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    OpenAI,
    Anthropic,
    Gemini,
    DeepSeek,
    Groq,
    MiniMax,
    Local,
    Zai,
    OpenCode,
    OpenRouter,
}

impl ProviderKind {
    /// As str.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::DeepSeek => "deepseek",
            Self::Groq => "groq",
            Self::MiniMax => "minimax",
            Self::Local => "local",
            Self::Zai => "z.ai",
            Self::OpenCode => "opencode",
            Self::OpenRouter => "openrouter",
        }
    }

    #[allow(clippy::should_implement_trait)]
    /// From str.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "openai" => Some(Self::OpenAI),
            "anthropic" => Some(Self::Anthropic),
            "gemini" => Some(Self::Gemini),
            "deepseek" => Some(Self::DeepSeek),
            "groq" => Some(Self::Groq),
            "minimax" => Some(Self::MiniMax),
            "local" => Some(Self::Local),
            "z.ai" | "zai" => Some(Self::Zai),
            "opencode" => Some(Self::OpenCode),
            "openrouter" => Some(Self::OpenRouter),
            _ => None,
        }
    }

    /// All.
    pub fn all() -> Vec<Self> {
        vec![
            Self::OpenAI,
            Self::Anthropic,
            Self::Gemini,
            Self::DeepSeek,
            Self::Groq,
            Self::MiniMax,
            Self::Local,
            Self::Zai,
            Self::OpenCode,
            Self::OpenRouter,
        ]
    }
}

/// Strategies for automatic provider selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoStrategy {
    LowestLatency,
    LowestCost,
    BestQuality,
    DeterministicOnly,
}

impl AutoStrategy {
    #[allow(clippy::should_implement_trait)]
    /// From str.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().replace("-", "").replace("_", "").as_str() {
            "lowestlatency" | "latency" => Some(Self::LowestLatency),
            "lowestcost" | "cost" => Some(Self::LowestCost),
            "bestquality" | "quality" => Some(Self::BestQuality),
            "deterministiconly" | "deterministic" => Some(Self::DeterministicOnly),
            _ => None,
        }
    }

    /// As str.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LowestLatency => "lowest-latency",
            Self::LowestCost => "lowest-cost",
            Self::BestQuality => "best-quality",
            Self::DeterministicOnly => "deterministic-only",
        }
    }
}

/// Represents the active provider configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ActiveProvider {
    Auto { strategy: AutoStrategy },
    Manual(ProviderKind),
    Fallback(Vec<ProviderKind>),
}

/// Records a provider switch event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSwitchEvent {
    pub from: Option<ProviderKind>,
    pub to: ProviderKind,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
}

/// Manages LLM provider selection and hot-switching.
pub struct ProviderRouter {
    active: ActiveProvider,
    fallback_chain: Vec<ProviderKind>,
    current_provider: ProviderKind,
    switch_history: Vec<ProviderSwitchEvent>,
    local_endpoints: Vec<String>,
    current_local_endpoint_idx: usize,
}

impl ProviderRouter {
    fn update_local_env(&self) {
        if self.current_provider == ProviderKind::Local {
            if let Some(endpoint) = self.current_local_endpoint() {
                std::env::set_var("XAVIER_LOCAL_LLM_URL", endpoint);
            }
        }
    }

    /// Checks if Ollama is reachable on the default local port.
    pub async fn is_ollama_reachable() -> bool {
        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap_or_default();

        let url = std::env::var("_XAVIER_TEST_OLLAMA_REACHABLE_URL").unwrap_or_else(|_| {
            crate::agents::provider::config::DEFAULT_LOCAL_BASE_URL.replace("/v1", "")
        });

        match client.get(&url).send().await {
            Ok(resp) => {
                resp.status().is_success() || resp.status() == reqwest::StatusCode::NOT_FOUND
            }
            Err(_) => false,
        }
    }

    /// Builds a default fallback chain based on configured providers and local availability.
    pub async fn build_default_chain(configured: &[ProviderKind]) -> Vec<ProviderKind> {
        let mut chain = Vec::new();

        // 1. Add cloud providers (everything except Local)
        for &p in configured {
            if p != ProviderKind::Local {
                chain.push(p);
            }
        }

        // 2. Add Local as last resort if Ollama is reachable
        if Self::is_ollama_reachable().await && !chain.contains(&ProviderKind::Local) {
            chain.push(ProviderKind::Local);
        }

        info!("Provider fallback chain built: {:?}", chain);
        chain
    }

    /// Creates a new ProviderRouter with the given initial provider.
    pub fn new(initial: ProviderKind) -> Self {
        let router = Self {
            active: ActiveProvider::Manual(initial),
            fallback_chain: Vec::new(),
            current_provider: initial,
            switch_history: vec![ProviderSwitchEvent {
                from: None,
                to: initial,
                reason: "Initialization".to_string(),
                timestamp: Utc::now(),
            }],
            local_endpoints: Vec::new(),
            current_local_endpoint_idx: 0,
        };
        router.update_local_env();
        router
    }

    /// Sets the ordered local endpoints for `ProviderKind::Local`.
    pub fn set_local_endpoints(&mut self, endpoints: Vec<String>) {
        self.local_endpoints = endpoints;
        self.current_local_endpoint_idx = 0;
        self.update_local_env();
    }

    /// Returns the active local endpoint, if any are configured and the current provider is Local.
    pub fn current_local_endpoint(&self) -> Option<String> {
        if self.current_provider == ProviderKind::Local && !self.local_endpoints.is_empty() {
            Some(self.local_endpoints[self.current_local_endpoint_idx].clone())
        } else {
            None
        }
    }

    /// Returns the list of configured local endpoints.
    pub fn local_endpoints(&self) -> &[String] {
        &self.local_endpoints
    }

    /// Returns the index of the currently active local endpoint.
    pub fn current_local_endpoint_idx(&self) -> usize {
        self.current_local_endpoint_idx
    }

    /// Switches the active provider manually.
    pub fn switch_to(&mut self, provider: ProviderKind) -> Result<()> {
        let event = ProviderSwitchEvent {
            from: Some(self.current_provider),
            to: provider,
            reason: "Manual switch".to_string(),
            timestamp: Utc::now(),
        };
        self.current_provider = provider;
        self.active = ActiveProvider::Manual(provider);
        self.switch_history.push(event);
        if provider == ProviderKind::Local {
            self.current_local_endpoint_idx = 0;
        }
        self.update_local_env();
        Ok(())
    }

    /// Configures the router to use an automatic selection strategy.
    pub fn set_auto_strategy(&mut self, strategy: AutoStrategy) {
        self.active = ActiveProvider::Auto { strategy };
        // Resolution of the actual provider based on strategy will happen in use cases
        // or via a dedicated resolve method that has access to metrics.
    }

    /// Sets the fallback chain for the router.
    pub fn set_fallback_chain(&mut self, providers: Vec<ProviderKind>) {
        self.fallback_chain = providers.clone();
        if matches!(self.active, ActiveProvider::Fallback(_)) {
            self.active = ActiveProvider::Fallback(providers);
        }
    }

    /// Handles a provider failure by switching to the next available fallback.
    pub fn on_provider_failure(&mut self) -> Option<ProviderKind> {
        // If current provider is Local, try next local endpoint first.
        if self.current_provider == ProviderKind::Local
            && !self.local_endpoints.is_empty()
            && self.current_local_endpoint_idx + 1 < self.local_endpoints.len()
        {
            self.current_local_endpoint_idx += 1;
            let next_endpoint = &self.local_endpoints[self.current_local_endpoint_idx];
            self.perform_switch(
                ProviderKind::Local,
                format!("Failure fallback (Next local endpoint: {})", next_endpoint),
            );
            self.update_local_env();
            return Some(ProviderKind::Local);
        }

        let (chain, is_transition) = match &self.active {
            ActiveProvider::Manual(_) | ActiveProvider::Auto { .. } => {
                if self.fallback_chain.is_empty() {
                    return None;
                }
                (self.fallback_chain.clone(), true)
            }
            ActiveProvider::Fallback(chain) => (chain.clone(), false),
        };

        let current_idx = chain
            .iter()
            .position(|&p| p == self.current_provider)
            .unwrap_or(usize::MAX);

        let next_idx = if current_idx == usize::MAX {
            0
        } else {
            current_idx + 1
        };

        if next_idx < chain.len() {
            let next = chain[next_idx];
            if next == ProviderKind::Local {
                self.current_local_endpoint_idx = 0;
            }
            if is_transition {
                self.active = ActiveProvider::Fallback(chain);
                self.perform_switch(
                    next,
                    "Failure fallback (Transition to Fallback mode)".to_string(),
                );
            } else {
                self.perform_switch(next, "Failure fallback (Chain)".to_string());
            }
            self.update_local_env();
            Some(next)
        } else {
            None
        }
    }

    fn perform_switch(&mut self, to: ProviderKind, reason: String) {
        let event = ProviderSwitchEvent {
            from: Some(self.current_provider),
            to,
            reason,
            timestamp: Utc::now(),
        };
        self.current_provider = to;
        self.switch_history.push(event);
    }

    /// Returns the currently active provider kind.
    pub fn current_provider(&self) -> ProviderKind {
        self.current_provider
    }

    /// Selects the best available provider (alias for current_provider).
    pub fn select(&self) -> ProviderKind {
        self.current_provider()
    }

    /// Handles a provider failure by switching to the next available fallback (alias for on_provider_failure).
    pub fn fallback(&mut self) -> Option<ProviderKind> {
        self.on_provider_failure()
    }

    /// Returns the current active configuration.
    pub fn active_mode(&self) -> &ActiveProvider {
        &self.active
    }

    /// Sets the active mode to Fallback using the current fallback chain.
    pub fn use_fallback_mode(&mut self) {
        self.active = ActiveProvider::Fallback(self.fallback_chain.clone());
        if let Some(&first) = self.fallback_chain.first() {
            if first == ProviderKind::Local {
                self.current_local_endpoint_idx = 0;
            }
            if first != self.current_provider {
                self.perform_switch(first, "Switch to fallback mode".to_string());
            }
            self.update_local_env();
        }
    }

    /// Returns the history of provider switches.
    pub fn history(&self) -> &[ProviderSwitchEvent] {
        &self.switch_history
    }

    /// Returns the current fallback chain.
    pub fn fallback_chain(&self) -> &[ProviderKind] {
        &self.fallback_chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_initialization() {
        let router = ProviderRouter::new(ProviderKind::Local);
        assert_eq!(router.current_provider(), ProviderKind::Local);
        assert_eq!(
            *router.active_mode(),
            ActiveProvider::Manual(ProviderKind::Local)
        );
        assert_eq!(router.history().len(), 1);
    }

    #[test]
    fn test_manual_switch() {
        let mut router = ProviderRouter::new(ProviderKind::Local);
        router.switch_to(ProviderKind::OpenAI).unwrap();
        assert_eq!(router.current_provider(), ProviderKind::OpenAI);
        assert_eq!(router.history().len(), 2);
    }

    #[test]
    fn test_fallback_on_failure() {
        let mut router = ProviderRouter::new(ProviderKind::OpenAI);
        router.set_fallback_chain(vec![
            ProviderKind::OpenAI,
            ProviderKind::Anthropic,
            ProviderKind::Local,
        ]);

        // Current is OpenAI (index 0).
        // First failure: should transition to Fallback mode and pick index 1 (Anthropic)
        let next = router.on_provider_failure();
        assert_eq!(next, Some(ProviderKind::Anthropic));
        assert_eq!(router.current_provider(), ProviderKind::Anthropic);
        assert!(matches!(router.active_mode(), ActiveProvider::Fallback(_)));

        // Second failure: should pick index 2 (Local)
        let next = router.on_provider_failure();
        assert_eq!(next, Some(ProviderKind::Local));
        assert_eq!(router.current_provider(), ProviderKind::Local);

        // Third failure: no more fallbacks
        let next = router.on_provider_failure();
        assert_eq!(next, None);
    }

    static TEST_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_build_default_chain_cloud_only() {
        let _guard = TEST_LOCK.lock().unwrap();
        // Since we can't easily mock network in unit tests without complex traits,
        // this test will depend on whether Ollama is actually running on the machine.
        // We'll just check that cloud providers are always included.
        let configured = vec![ProviderKind::OpenAI, ProviderKind::Anthropic];
        let chain = ProviderRouter::build_default_chain(&configured).await;

        assert!(chain.contains(&ProviderKind::OpenAI));
        assert!(chain.contains(&ProviderKind::Anthropic));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_build_default_chain_mixed() {
        let _guard = TEST_LOCK.lock().unwrap();
        let configured = vec![ProviderKind::OpenAI, ProviderKind::Local];
        let chain = ProviderRouter::build_default_chain(&configured).await;

        assert!(chain.contains(&ProviderKind::OpenAI));
        // Local might or might not be there depending on reachability,
        // but if it is, it should be at the end.
        if let Some(pos) = chain.iter().position(|&p| p == ProviderKind::Local) {
            assert_eq!(pos, chain.len() - 1);
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_provider_router_build_openai() {
        let _guard = TEST_LOCK.lock().unwrap();
        let configured = vec![];
        let chain = ProviderRouter::build_default_chain(&configured).await;
        // Should only contain Local if reachable, otherwise empty
        if !chain.is_empty() {
            assert_eq!(chain, vec![ProviderKind::Local]);
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_build_default_chain_with_mocked_ollama_reachable() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/")
            .with_status(200)
            .create_async()
            .await;

        std::env::set_var("_XAVIER_TEST_OLLAMA_REACHABLE_URL", server.url());

        let configured = vec![ProviderKind::OpenAI, ProviderKind::Local];
        let chain = ProviderRouter::build_default_chain(&configured).await;

        std::env::remove_var("_XAVIER_TEST_OLLAMA_REACHABLE_URL");

        assert_eq!(chain, vec![ProviderKind::OpenAI, ProviderKind::Local]);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_build_default_chain_with_mocked_ollama_unreachable() {
        let _guard = TEST_LOCK.lock().unwrap();
        std::env::set_var("_XAVIER_TEST_OLLAMA_REACHABLE_URL", "http://127.0.0.1:1");

        let configured = vec![ProviderKind::OpenAI, ProviderKind::Local];
        let chain = ProviderRouter::build_default_chain(&configured).await;

        std::env::remove_var("_XAVIER_TEST_OLLAMA_REACHABLE_URL");

        assert_eq!(chain, vec![ProviderKind::OpenAI]);
    }

    #[test]
    fn test_auto_strategy_setting() {
        let mut router = ProviderRouter::new(ProviderKind::Local);
        router.set_auto_strategy(AutoStrategy::LowestCost);
        assert_eq!(
            *router.active_mode(),
            ActiveProvider::Auto {
                strategy: AutoStrategy::LowestCost
            }
        );
    }

    #[test]
    fn test_local_endpoints_initialization_and_getters() {
        let orig_env = std::env::var("XAVIER_LOCAL_LLM_URL").ok();

        let mut router = ProviderRouter::new(ProviderKind::Local);
        router.set_local_endpoints(vec![
            "http://local-1".to_string(),
            "http://local-2".to_string(),
        ]);

        assert_eq!(
            router.local_endpoints(),
            &["http://local-1", "http://local-2"]
        );
        assert_eq!(router.current_local_endpoint_idx(), 0);
        assert_eq!(
            router.current_local_endpoint(),
            Some("http://local-1".to_string())
        );
        assert_eq!(
            std::env::var("XAVIER_LOCAL_LLM_URL").unwrap(),
            "http://local-1"
        );

        if let Some(val) = orig_env {
            std::env::set_var("XAVIER_LOCAL_LLM_URL", val);
        } else {
            std::env::remove_var("XAVIER_LOCAL_LLM_URL");
        }
    }

    #[test]
    fn test_local_endpoints_fallback_cycling() {
        let orig_env = std::env::var("XAVIER_LOCAL_LLM_URL").ok();
        std::env::remove_var("XAVIER_LOCAL_LLM_URL");

        let mut router = ProviderRouter::new(ProviderKind::OpenAI);
        router.set_fallback_chain(vec![
            ProviderKind::OpenAI,
            ProviderKind::Local,
            ProviderKind::Anthropic,
        ]);
        router.set_local_endpoints(vec![
            "http://local-1".to_string(),
            "http://local-2".to_string(),
        ]);

        // OpenAI fails -> fallback to Local (first endpoint)
        let next = router.on_provider_failure();
        assert_eq!(next, Some(ProviderKind::Local));
        assert_eq!(router.current_provider(), ProviderKind::Local);
        assert_eq!(router.current_local_endpoint_idx(), 0);
        assert_eq!(
            router.current_local_endpoint(),
            Some("http://local-1".to_string())
        );
        assert_eq!(
            std::env::var("XAVIER_LOCAL_LLM_URL").unwrap(),
            "http://local-1"
        );

        // Local fails -> fallback to Local (second endpoint)
        let next = router.on_provider_failure();
        assert_eq!(next, Some(ProviderKind::Local));
        assert_eq!(router.current_provider(), ProviderKind::Local);
        assert_eq!(router.current_local_endpoint_idx(), 1);
        assert_eq!(
            router.current_local_endpoint(),
            Some("http://local-2".to_string())
        );
        assert_eq!(
            std::env::var("XAVIER_LOCAL_LLM_URL").unwrap(),
            "http://local-2"
        );

        // Local fails (no more endpoints) -> fallback to Anthropic
        let next = router.on_provider_failure();
        assert_eq!(next, Some(ProviderKind::Anthropic));
        assert_eq!(router.current_provider(), ProviderKind::Anthropic);
        assert_eq!(router.current_local_endpoint(), None);

        if let Some(val) = orig_env {
            std::env::set_var("XAVIER_LOCAL_LLM_URL", val);
        } else {
            std::env::remove_var("XAVIER_LOCAL_LLM_URL");
        }
    }

    #[test]
    fn test_local_endpoints_manual_switch_resets_index() {
        let orig_env = std::env::var("XAVIER_LOCAL_LLM_URL").ok();

        let mut router = ProviderRouter::new(ProviderKind::OpenAI);
        router.set_local_endpoints(vec![
            "http://local-1".to_string(),
            "http://local-2".to_string(),
        ]);

        // Manually switch to Local
        router.switch_to(ProviderKind::Local).unwrap();
        assert_eq!(router.current_local_endpoint_idx(), 0);
        assert_eq!(
            router.current_local_endpoint(),
            Some("http://local-1".to_string())
        );

        // Advance to index 1 manually by triggering failure
        let next = router.on_provider_failure();
        assert_eq!(next, Some(ProviderKind::Local));
        assert_eq!(router.current_local_endpoint_idx(), 1);

        // Switch to OpenAI
        router.switch_to(ProviderKind::OpenAI).unwrap();
        // Index is reset upon switching manually to Local, switch_to to OpenAI won't immediately reset it,
        // but let's see. When we switch manually back to Local, it should reset to 0.

        // Switch back to Local
        router.switch_to(ProviderKind::Local).unwrap();
        assert_eq!(router.current_local_endpoint_idx(), 0);

        if let Some(val) = orig_env {
            std::env::set_var("XAVIER_LOCAL_LLM_URL", val);
        } else {
            std::env::remove_var("XAVIER_LOCAL_LLM_URL");
        }
    }
}
