//! Provider router for hot-switching LLM providers.
//!
//! This module implements the logic for switching providers at runtime,
//! managing fallback chains, and automatic strategy-based provider selection.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::DeepSeek => "deepseek",
            Self::Groq => "groq",
            Self::MiniMax => "minimax",
            Self::Local => "local",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "openai" => Some(Self::OpenAI),
            "anthropic" => Some(Self::Anthropic),
            "gemini" => Some(Self::Gemini),
            "deepseek" => Some(Self::DeepSeek),
            "groq" => Some(Self::Groq),
            "minimax" => Some(Self::MiniMax),
            "local" => Some(Self::Local),
            _ => None,
        }
    }

    pub fn all() -> Vec<Self> {
        vec![
            Self::OpenAI,
            Self::Anthropic,
            Self::Gemini,
            Self::DeepSeek,
            Self::Groq,
            Self::MiniMax,
            Self::Local,
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
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().replace("-", "").replace("_", "").as_str() {
            "lowestlatency" | "latency" => Some(Self::LowestLatency),
            "lowestcost" | "cost" => Some(Self::LowestCost),
            "bestquality" | "quality" => Some(Self::BestQuality),
            "deterministiconly" | "deterministic" => Some(Self::DeterministicOnly),
            _ => None,
        }
    }

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
}

impl ProviderRouter {
    /// Creates a new ProviderRouter with the given initial provider.
    pub fn new(initial: ProviderKind) -> Self {
        Self {
            active: ActiveProvider::Manual(initial),
            fallback_chain: Vec::new(),
            current_provider: initial,
            switch_history: vec![ProviderSwitchEvent {
                from: None,
                to: initial,
                reason: "Initialization".to_string(),
                timestamp: Utc::now(),
            }],
        }
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
            if is_transition {
                self.active = ActiveProvider::Fallback(chain);
                self.perform_switch(
                    next,
                    "Failure fallback (Transition to Fallback mode)".to_string(),
                );
            } else {
                self.perform_switch(next, "Failure fallback (Chain)".to_string());
            }
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
            if first != self.current_provider {
                self.perform_switch(first, "Switch to fallback mode".to_string());
            }
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
        assert_eq!(*router.active_mode(), ActiveProvider::Manual(ProviderKind::Local));
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

    #[test]
    fn test_auto_strategy_setting() {
        let mut router = ProviderRouter::new(ProviderKind::Local);
        router.set_auto_strategy(AutoStrategy::LowestCost);
        assert_eq!(*router.active_mode(), ActiveProvider::Auto { strategy: AutoStrategy::LowestCost });
    }
}
