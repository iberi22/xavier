//! Core types for the plugin system.
//!
//! These types describe plugin descriptors, the per-language fallback chain,
//! the stdin/stdout JSON protocol used to talk to plugin processes, and the
//! traits the engine/registry/health layers will implement in later phases.
//!
//! This module is dependency-light on purpose: it only pulls in `serde` and the
//! crate's own [`crate::types`] so it can be referenced from the engine, the
//! fallback chain, and the (deprecated) `plugin_host` shim without cycles.

use crate::error::{GraphError, Result};
use crate::types::{Language, Symbol};
use serde::{Deserialize, Serialize};

// ============================================================================
// Plugin descriptor
// ============================================================================

/// Static description of an installed plugin.
///
/// Mirrors a single entry in the legacy `plugins.json` config file so the
/// deprecated [`crate::plugin_host::PluginHost`] can keep loading it unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    /// Executable invoked for a parse request (path or `$PATH`-resolvable name).
    pub command: String,
    /// Semver-ish version string, informational only at this phase.
    pub version: String,
    /// Languages this plugin can parse.
    pub languages: Vec<Language>,
    /// Extensions handled by this plugin (e.g. `["rb", "lua"]`).
    pub extensions: Option<Vec<String>>,
    /// Operations the plugin advertises, e.g. `["parse"]`.
    pub capabilities: Vec<String>,
}

/// Richer descriptor used by the registry/lifecycle phases (F3+). Kept here so
/// the engine and fallback chain can reason about an installed plugin uniformly
/// even before the registry exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDescriptor {
    pub name: String,
    pub version: String,
    pub command: String,
    pub languages: Vec<Language>,
    pub extensions: Vec<String>,
    pub capabilities: Vec<String>,
}

impl From<&PluginConfig> for PluginDescriptor {
    fn from(cfg: &PluginConfig) -> Self {
        // Derive a stable name from the command basename when one isn't given.
        let name = cfg
            .command
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&cfg.command)
            .to_string();
        PluginDescriptor {
            name,
            version: cfg.version.clone(),
            command: cfg.command.clone(),
            languages: cfg.languages.clone(),
            extensions: cfg.extensions.clone().unwrap_or_default(),
            capabilities: cfg.capabilities.clone(),
        }
    }
}

impl PluginDescriptor {
    /// True when the plugin advertises the `parse` capability for `lang`.
    pub fn supports(&self, lang: &Language, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
            && self.languages.iter().any(|l| l == lang)
    }
}

// ============================================================================
// Fallback chain
// ============================================================================

/// A single ordered step in a per-language fallback chain.
///
/// `Plugin(String)` carries the plugin *name* (not command) so the engine can
/// resolve the descriptor via [`crate::plugin::PluginManager`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FallbackStep {
    /// Try the named plugin first.
    Plugin(String),
    /// Fall back to the built-in tree-sitter parser.
    Native,
    /// Give up and emit an empty symbol list (never errors).
    NoOp,
}

// ============================================================================
// stdin/stdout JSON protocol
// ============================================================================

/// Request sent to a plugin process over stdin.
#[derive(Debug, Serialize, Deserialize)]
pub struct PluginRequest {
    pub language: Language,
    pub files: Vec<FileToParse>,
}

/// A single file to parse inside a [`PluginRequest`].
#[derive(Debug, Serialize, Deserialize)]
pub struct FileToParse {
    pub path: String,
    pub source: String,
}

/// Response read from a plugin process over stdout.
#[derive(Debug, Serialize, Deserialize)]
pub struct PluginResponse {
    pub symbols: Vec<Symbol>,
    pub error: Option<String>,
}

impl PluginResponse {
    /// Convert a response into a `Result`, surfacing the plugin-reported error.
    pub fn into_result(self) -> Result<Vec<Symbol>> {
        if let Some(err) = self.error {
            return Err(GraphError::Parser(err));
        }
        Ok(self.symbols)
    }
}

// ============================================================================
// Health (minimal; ring buffer + circuit breaker land in F4)
// ============================================================================

/// Lightweight health record accumulated per plugin during this phase.
///
/// The full `MetricsRingBuffer` / circuit-breaker (3 fails in 60s → auto-disable)
/// from the feature spec is deferred to phase 4; for now we track aggregate
/// counters so the engine can emit useful warnings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginHealth {
    pub success_count: u64,
    pub failure_count: u64,
    pub last_error: Option<String>,
}

impl PluginHealth {
    pub fn record_success(&mut self) {
        self.success_count += 1;
        self.last_error = None;
    }

    pub fn record_failure(&mut self, error: impl Into<String>) {
        self.failure_count += 1;
        self.last_error = Some(error.into());
    }
}

// ============================================================================
// Traits (shapes the engine/registry/health phases implement later)
// ============================================================================

/// Executes a parse request against a plugin process.
///
/// Implemented by [`crate::plugin::engine::ProcessEngine`].
pub trait PluginEngine: Send + Sync {
    fn parse(
        &self,
        config: &PluginConfig,
        lang: Language,
        files: Vec<FileToParse>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Symbol>>> + Send>>;
}

/// Resolves which fallback chain applies to a given language.
///
/// Implemented by [`crate::plugin::fallback::FallbackChain`].
pub trait FallbackResolver: Send + Sync {
    fn chain_for(&self, lang: &Language) -> Vec<FallbackStep>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_supports_advertised_language() {
        let desc = PluginDescriptor {
            name: "parser-py".into(),
            version: "1.0.0".into(),
            command: "parser-py".into(),
            languages: vec![Language::Python],
            extensions: vec!["py".into()],
            capabilities: vec!["parse".into()],
        };
        assert!(desc.supports(&Language::Python, "parse"));
        assert!(!desc.supports(&Language::Rust, "parse"));
        assert!(!desc.supports(&Language::Python, "health"));
    }

    #[test]
    fn fallback_step_serializes_stably() {
        let step = FallbackStep::Plugin("parser-py".into());
        let json = serde_json::to_string(&step).expect("serialize");
        assert_eq!(json, r#"{"Plugin":"parser-py"}"#);
        let back: FallbackStep = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, step);
    }

    #[test]
    fn plugin_response_into_result_surfaces_error() {
        let resp = PluginResponse {
            symbols: vec![],
            error: Some("boom".into()),
        };
        let err = resp.into_result().unwrap_err();
        assert!(matches!(err, GraphError::Parser(_)));
    }
}
