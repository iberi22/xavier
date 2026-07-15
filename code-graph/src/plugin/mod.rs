//! Plugin system for code-graph.
//!
//! Extracts language-specific parsing into externally-managed plugins so a
//! crashed parser never takes down the indexer. Per the feature spec, parse
//! requests are routed through a per-language **fallback chain**:
//!
//! ```text
//! Plugin → Native (tree-sitter) → NoOp (empty symbols)
//! ```
//!
//! Phase scope (this file + siblings):
//! - [`types`]   — descriptors, `FallbackStep`, protocol types, traits.
//! - [`engine`]  — `ProcessEngine` running a plugin as an isolated subprocess.
//! - [`fallback`]— `FallbackChain` + persistence to `fallback.json`.
//!
//! Deferred to later phases: GitHub registry, archive extraction, lifecycle
//! (install/update/rollback), CLI, health-monitor ring buffer, discovery.

pub mod discovery;
pub mod engine;
pub mod fallback;
pub mod types;

use crate::error::{GraphError, Result};
use crate::types::{Language, LanguageDiscovery, Symbol};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

pub use engine::ProcessEngine;
pub use fallback::FallbackChain;
pub use types::{
    FallbackResolver, FallbackStep, FileToParse, InstalledPlugin, PluginConfig, PluginDescriptor,
    PluginEngine, PluginHealth, PluginRequest, PluginResponse,
};

/// Orchestrates plugin lifecycle and execution for the indexer.
///
/// Holds the installed-plugin table (language → descriptor), the fallback
/// chain resolver, and the process engine. The registry/lifecycle phases
/// (F3+) will add install/update/rollback on top of this skeleton.
pub struct PluginManager {
    /// Plugins keyed by the language they handle (first-wins on collision).
    installed: RwLock<HashMap<Language, PluginDescriptor>>,
    /// Plugins keyed by descriptor name, for `FallbackStep::Plugin(name)`.
    by_name: RwLock<HashMap<String, PluginDescriptor>>,
    fallback: RwLock<FallbackChain>,
    engine: Arc<dyn PluginEngine>,
}

impl PluginManager {
    /// Build an empty manager with default fallback chains and the standard
    /// subprocess engine.
    pub fn new() -> Self {
        Self::with_engine(Arc::new(ProcessEngine::default()))
    }

    /// Build a manager with a custom engine (used by tests / future WASM engine).
    pub fn with_engine(engine: Arc<dyn PluginEngine>) -> Self {
        let installed: HashMap<Language, PluginDescriptor> = HashMap::new();
        let fallback = FallbackChain::load_or_default();
        Self {
            installed: RwLock::new(installed.clone()),
            by_name: RwLock::new(HashMap::new()),
            fallback: RwLock::new(fallback),
            engine,
        }
    }

    /// Register a plugin descriptor. Subsequent fallback-chain lookups for any
    /// of its languages will prefer the plugin before the native parser.
    pub fn register(&self, descriptor: PluginDescriptor) {
        let name = descriptor.name.clone();
        let langs = descriptor.languages.clone();
        {
            let mut by_name = self.by_name.write();
            by_name.insert(name.clone(), descriptor);
        }
        let mut installed = self.installed.write();
        for lang in langs {
            // First-wins: don't silently displace an already-registered plugin.
            installed.entry(lang).or_insert_with(|| {
                self.by_name
                    .read()
                    .get(&name)
                    .expect("just inserted")
                    .clone()
            });
        }
    }

    /// Look up the descriptor registered for a language, if any.
    pub fn descriptor_for(&self, lang: &Language) -> Option<PluginDescriptor> {
        self.installed.read().get(lang).cloned()
    }

    /// Look up a descriptor by plugin name (used by `FallbackStep::Plugin(name)`).
    pub fn descriptor_by_name(&self, name: &str) -> Option<PluginDescriptor> {
        self.by_name.read().get(name).cloned()
    }

    /// List all installed plugins.
    pub fn list(&self) -> Vec<InstalledPlugin> {
        self.by_name
            .read()
            .values()
            .map(|desc| InstalledPlugin {
                version: desc.version.clone(),
                descriptor: desc.clone(),
                // In this phase, we don't have multiple cached versions yet.
                cached_versions: vec![desc.version.clone()],
            })
            .collect()
    }

    /// Resolve the fallback chain for a language (plugin-first if one is
    /// installed, otherwise native → NoOp).
    pub fn chain_for(&self, lang: &Language) -> Vec<FallbackStep> {
        // A plugin installed for this language always wins over a persisted
        // fallback config, so resolution is live-state-aware.
        if self.installed.read().contains_key(lang) {
            if let Some(desc) = self.installed.read().get(lang) {
                return vec![
                    FallbackStep::Plugin(desc.name.clone()),
                    FallbackStep::Native,
                    FallbackStep::NoOp,
                ];
            }
        }
        self.fallback.read().chain_for(lang)
    }

    /// Execute a parse request against a named plugin.
    ///
    /// Returns `Err` if the plugin is unknown or the subprocess fails; callers
    /// (the fallback driver) are expected to log + continue on `Err`.
    pub async fn parse_with_plugin(
        &self,
        name: &str,
        lang: Language,
        files: Vec<FileToParse>,
    ) -> Result<Vec<Symbol>> {
        let descriptor = self
            .descriptor_by_name(name)
            .ok_or_else(|| GraphError::Parser(format!("unknown plugin '{}'", name)))?;
        let config = PluginConfig {
            command: descriptor.command,
            version: descriptor.version,
            languages: descriptor.languages,
            extensions: Some(descriptor.extensions),
            capabilities: descriptor.capabilities,
        };
        self.engine.parse(&config, lang, files).await
    }

    /// Load plugins from the legacy `plugins.json` config file, if present.
    ///
    /// Mirrors the previous `PluginHost::load_plugins` behaviour so the
    /// deprecated wrapper keeps working transparently.
    pub fn load_config(&self) -> Result<()> {
        let Some(config_dir) = dirs::config_dir() else {
            return Ok(());
        };
        let plugins_json = config_dir.join("code-graph").join("plugins.json");
        if !plugins_json.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&plugins_json).map_err(GraphError::Io)?;
        let configs: Vec<PluginConfig> =
            serde_json::from_str(&content).map_err(|e| GraphError::Parser(e.to_string()))?;

        for config in configs {
            debug!(?config.command, "Registering plugin from config");
            self.register(PluginDescriptor::from(&config));
        }
        Ok(())
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageDiscovery for PluginManager {
    fn language_for_extension(&self, ext: &str) -> Language {
        let ext = ext.to_lowercase();
        let installed = self.installed.read();
        for (lang, desc) in installed.iter() {
            if desc.extensions.iter().any(|e| e.to_lowercase() == ext) {
                return lang.clone();
            }
        }
        Language::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_prefers_installed_plugin_for_a_language() {
        let manager = PluginManager::new();
        assert_eq!(
            manager.chain_for(&Language::Python),
            vec![FallbackStep::Native, FallbackStep::NoOp],
        );

        manager.register(PluginDescriptor {
            name: "parser-py".into(),
            version: "1.0.0".into(),
            command: "parser-py".into(),
            languages: vec![Language::Python],
            extensions: vec!["py".into()],
            capabilities: vec!["parse".into()],
        });

        assert_eq!(
            manager.chain_for(&Language::Python),
            vec![
                FallbackStep::Plugin("parser-py".into()),
                FallbackStep::Native,
                FallbackStep::NoOp,
            ],
        );
        // Untouched language keeps the default chain.
        assert_eq!(
            manager.chain_for(&Language::Rust),
            vec![FallbackStep::Native, FallbackStep::NoOp],
        );
    }

    #[test]
    fn descriptor_for_and_by_name_resolve_after_register() {
        let manager = PluginManager::new();
        manager.register(PluginDescriptor {
            name: "parser-py".into(),
            version: "1.0.0".into(),
            command: "/usr/bin/parser-py".into(),
            languages: vec![Language::Python],
            extensions: vec!["py".into()],
            capabilities: vec!["parse".into()],
        });
        assert_eq!(
            manager
                .descriptor_for(&Language::Python)
                .map(|d| d.name)
                .as_deref(),
            Some("parser-py"),
        );
        assert_eq!(
            manager
                .descriptor_by_name("parser-py")
                .map(|d| d.command)
                .as_deref(),
            Some("/usr/bin/parser-py"),
        );
    }
}
