//! Deprecated plugin host — thin wrapper over [`crate::plugin::PluginManager`].
//!
//! Historical note: this module used to own plugin loading, dispatch, and the
//! subprocess protocol. All of that now lives under [`crate::plugin`]
//! (`PluginManager`, `ProcessEngine`, `FallbackChain`). This file keeps the old
//! API alive for the `Indexer` and any external callers, delegating to the new
//! implementation. New code should use [`crate::plugin::PluginManager`] directly.
//!
//! Bug fixes carried over from the legacy implementation:
//! - `Language::Rust` is no longer hard-wired to `Native`; it goes through the
//!   fallback chain like every other language.
//! - Protocol types (`PluginConfig`, `PluginRequest`, `PluginResponse`,
//!   `FileToParse`) are re-exported from [`crate::plugin::types`].

use crate::error::Result;
use crate::plugin::types::{FileToParse, PluginConfig};
use crate::plugin::PluginManager;
use crate::types::{Language, Symbol};
use std::sync::Arc;

// Re-export the protocol types so existing `use crate::plugin_host::{...}`
// sites keep resolving without touching them.
pub use crate::plugin::types::{PluginRequest, PluginResponse};

/// Legacy dispatch decision retained for backwards compatibility.
///
/// New code should inspect the [`crate::plugin::types::FallbackStep`] chain
/// from [`PluginManager::chain_for`] instead.
#[derive(Debug)]
#[deprecated(note = "Use PluginManager::chain_for / FallbackStep")]
pub enum ParserDispatch {
    Native,
    Plugin(PluginConfig),
    NoOp,
}

/// Backwards-compatible wrapper around [`PluginManager`].
#[deprecated(note = "Use crate::plugin::PluginManager directly")]
pub struct PluginHost {
    manager: Arc<PluginManager>,
}

#[allow(deprecated)]
impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(deprecated)]
impl PluginHost {
    pub fn new() -> Self {
        let manager = PluginManager::new();
        // Load legacy plugins.json so existing operator configs still apply.
        if let Err(e) = manager.load_config() {
            tracing::debug!("Failed to load legacy plugin config: {}", e);
        }
        Self {
            manager: Arc::new(manager),
        }
    }

    /// Access the underlying manager (for callers migrating off the wrapper).
    pub fn manager(&self) -> &PluginManager {
        &self.manager
    }

    /// Legacy dispatch picker. Honours the fallback chain rather than the old
    /// Rust-hardcoded-to-Native shortcut.
    #[allow(deprecated)]
    pub fn parser_for(&self, lang: &Language) -> ParserDispatch {
        use crate::plugin::types::FallbackStep;
        let chain = self.manager.chain_for(lang);
        match chain.first() {
            Some(FallbackStep::Plugin(name)) => {
                if let Some(desc) = self.manager.descriptor_by_name(name) {
                    return ParserDispatch::Plugin(PluginConfig {
                        name: desc.name,
                        command: desc.command,
                        version: desc.version,
                        languages: desc.languages,
                        capabilities: desc.capabilities,
                    });
                }
                // Plugin referenced but not registered — fall through to next step.
                if chain.iter().nth(1).is_some() {
                    return ParserDispatch::Native;
                }
                ParserDispatch::NoOp
            }
            Some(FallbackStep::Native) => ParserDispatch::Native,
            _ => ParserDispatch::NoOp,
        }
    }

    /// Run a plugin parse. Delegates to the shared engine.
    pub async fn parse_with_plugin(
        &self,
        config: &PluginConfig,
        lang: Language,
        files: Vec<FileToParse>,
    ) -> Result<Vec<Symbol>> {
        // Register the config on the fly so the engine can resolve it by name.
        // (Idempotent: re-registering the same language is first-wins.)
        let descriptor = crate::plugin::types::PluginDescriptor::from(config);
        let name = descriptor.name.clone();
        self.manager.register(descriptor);
        self.manager.parse_with_plugin(&name, lang, files).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Language;
    use std::collections::HashMap;

    #[test]
    #[allow(deprecated)]
    fn plugin_host_delegates_to_manager_and_drops_rust_hardcode() {
        let host = PluginHost::new();

        // Rust used to short-circuit to Native; now it follows the chain.
        match host.parser_for(&Language::Rust) {
            ParserDispatch::Native => (),
            other => panic!("expected Native for Rust, got {:?}", other),
        }
        match host.parser_for(&Language::Unknown) {
            ParserDispatch::NoOp => (),
            other => panic!("expected NoOp for Unknown, got {:?}", other),
        }

        // Register a plugin and confirm the host surfaces it.
        host.manager
            .register(crate::plugin::types::PluginDescriptor {
                name: "parser-py".to_string(),
                version: "1.0.0".to_string(),
                command: "parser-py".to_string(),
                languages: vec![Language::Python],
                capabilities: vec!["parse".to_string()],
            });
        match host.parser_for(&Language::Python) {
            ParserDispatch::Plugin(c) => assert_eq!(c.command, "parser-py"),
            other => panic!("expected Plugin for Python, got {:?}", other),
        }

        // Silence unused-import noise from the legacy HashMap import.
        let _ = HashMap::<Language, ()>::new();
    }
}
