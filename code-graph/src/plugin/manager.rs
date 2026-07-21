//! Plugin manager for code-graph.
//!
//! Orchestrates plugin lifecycle and execution for the indexer.
//!
//! Holds the installed-plugin table (language → descriptor), the fallback
//! chain resolver, and the process engine.

use crate::error::{GraphError, Result};
use crate::plugin::engine::ProcessEngine;
use crate::plugin::fallback::FallbackChain;
use crate::plugin::health::PluginHealthMonitor;
use crate::plugin::types::{
    FallbackResolver, FallbackStep, FileToParse, PluginConfig, PluginDescriptor, PluginEngine,
    PluginRegistry, RegistryEntry,
};
use crate::types::{Language, LanguageDiscovery, Symbol};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

/// Orchestrates plugin lifecycle and execution for the indexer.
pub struct PluginManager {
    /// Plugins keyed by the language they handle (first-wins on collision).
    installed: RwLock<HashMap<Language, PluginDescriptor>>,
    /// Plugins keyed by descriptor name, for `FallbackStep::Plugin(name)`.
    by_name: RwLock<HashMap<String, PluginDescriptor>>,
    fallback: RwLock<FallbackChain>,
    engine: Arc<dyn PluginEngine>,
    registry: Arc<dyn PluginRegistry>,
    health: RwLock<Option<Arc<PluginHealthMonitor>>>,
}

impl PluginManager {
    /// Build an empty manager with default fallback chains and the standard
    /// subprocess engine.
    pub fn new() -> Self {
        let engine = Arc::new(ProcessEngine::default());
        let registry = Arc::new(DefaultRegistry::new());
        let health = Arc::new(PluginHealthMonitor::new(std::time::Duration::from_secs(60)));
        engine.set_monitor(Arc::clone(&health));

        let installed: HashMap<Language, PluginDescriptor> = HashMap::new();
        let fallback = FallbackChain::load_or_default();
        let manager = Self {
            installed: RwLock::new(installed),
            by_name: RwLock::new(HashMap::new()),
            fallback: RwLock::new(fallback),
            engine,
            registry,
            health: RwLock::new(Some(health)),
        };

        if which::which("codegraph").is_ok() {
            let descriptor = PluginDescriptor {
                name: "codegraph".to_string(),
                version: "1.4.1".to_string(),
                command: "codegraph".to_string(),
                languages: vec![
                    Language::Rust,
                    Language::TypeScript,
                    Language::Python,
                    Language::Go,
                    Language::Java,
                    Language::C,
                    Language::Cpp,
                ],
                extensions: vec![
                    "rs".to_string(),
                    "ts".to_string(),
                    "tsx".to_string(),
                    "py".to_string(),
                    "go".to_string(),
                    "java".to_string(),
                    "c".to_string(),
                    "h".to_string(),
                    "cpp".to_string(),
                    "cc".to_string(),
                    "cxx".to_string(),
                    "hpp".to_string(),
                ],
                capabilities: vec![
                    "parse".to_string(),
                    "index".to_string(),
                    "query".to_string(),
                ],
            };
            manager.register(descriptor);
        }

        manager
    }

    /// Build a manager with a custom engine and registry.
    pub fn with_engine_and_registry(
        engine: Arc<dyn PluginEngine>,
        registry: Arc<dyn PluginRegistry>,
    ) -> Self {
        let installed: HashMap<Language, PluginDescriptor> = HashMap::new();
        let fallback = FallbackChain::load_or_default();
        Self {
            installed: RwLock::new(installed),
            by_name: RwLock::new(HashMap::new()),
            fallback: RwLock::new(fallback),
            engine,
            registry,
            health: RwLock::new(None),
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
            name: descriptor.name.clone(),
            command: descriptor.command,
            version: descriptor.version,
            languages: descriptor.languages,
            extensions: Some(descriptor.extensions),
            capabilities: descriptor.capabilities,
        };
        self.engine.parse(&config, lang, files).await
    }

    /// Load plugins from the legacy `plugins.json` config file.
    pub fn load_config(&self) -> Result<()> {
        if cfg!(test) {
            return Ok(());
        }
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

    // ========================================================================
    // Lifecycle (F4 endpoints)
    // ========================================================================

    pub fn list(&self) -> Vec<PluginDescriptor> {
        self.by_name.read().values().cloned().collect()
    }

    pub async fn install(&self, name: &str, version: Option<String>) -> Result<PluginDescriptor> {
        // Stub for F4
        let entry = self.registry.get(name).await?;
        let desc = PluginDescriptor {
            name: entry.name,
            version: version.unwrap_or(entry.version),
            command: "stub".to_string(), // In real impl, this would be the downloaded path
            languages: entry.languages,
            extensions: vec![],
            capabilities: entry.capabilities,
        };
        self.register(desc.clone());
        Ok(desc)
    }

    pub async fn update(&self, name: Option<String>) -> Result<Vec<PluginDescriptor>> {
        // Stub for F4
        if let Some(n) = name {
            let desc = self.install(&n, None).await?;
            Ok(vec![desc])
        } else {
            Ok(vec![])
        }
    }

    pub async fn rollback(&self, name: &str) -> Result<PluginDescriptor> {
        // Stub for F4
        self.descriptor_by_name(name)
            .ok_or_else(|| GraphError::Parser(format!("Plugin not found: {}", name)))
    }

    pub async fn uninstall(&self, name: &str) -> Result<()> {
        let mut by_name = self.by_name.write();
        if let Some(desc) = by_name.remove(name) {
            let mut installed = self.installed.write();
            for lang in desc.languages {
                if let Some(current) = installed.get(&lang) {
                    if current.name == name {
                        installed.remove(&lang);
                    }
                }
            }
            Ok(())
        } else {
            Err(GraphError::Parser(format!("Plugin not found: {}", name)))
        }
    }

    pub fn registry(&self) -> Arc<dyn PluginRegistry> {
        Arc::clone(&self.registry)
    }

    pub fn fallback(&self) -> &RwLock<FallbackChain> {
        &self.fallback
    }

    pub fn health(&self) -> Option<Arc<PluginHealthMonitor>> {
        self.health.read().clone()
    }

    pub fn all_plugin_names(&self) -> Vec<String> {
        self.by_name.read().keys().cloned().collect()
    }
}

impl LanguageDiscovery for PluginManager {
    fn language_for_extension(&self, ext: &str) -> Language {
        let ext_lower = ext.to_lowercase();
        let by_name = self.by_name.read();
        for desc in by_name.values() {
            if desc
                .extensions
                .iter()
                .any(|e| e.to_lowercase() == ext_lower)
            {
                if let Some(lang) = desc.languages.first() {
                    return lang.clone();
                }
            }
        }
        Language::Unknown
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Default Registry (fixture / env-backed index)
// ============================================================================

/// Loads a registry index from disk.
///
/// Resolution order:
/// 1. `XAVIER_PLUGINS_INDEX` env (path to `plugins.json`)
/// 2. In-repo fixture `code-graph/fixtures/xavier-plugins/plugins.json`
///    (relative to CARGO_MANIFEST_DIR when built, else cwd walk)
///
/// Production can point `XAVIER_PLUGINS_INDEX` at a live remote mirror path.
struct DefaultRegistry {
    entries: Vec<RegistryEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct RegistryIndexFile {
    plugins: Vec<RegistryEntry>,
}

impl DefaultRegistry {
    fn new() -> Self {
        let entries = Self::load_entries();
        Self { entries }
    }

    fn load_entries() -> Vec<RegistryEntry> {
        let candidates: Vec<std::path::PathBuf> = {
            let mut paths = Vec::new();
            if let Ok(p) = std::env::var("XAVIER_PLUGINS_INDEX") {
                paths.push(std::path::PathBuf::from(p));
            }
            // Built-in fixture (crate root when compiling code-graph)
            paths.push(
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("fixtures/xavier-plugins/plugins.json"),
            );
            // Workspace-relative fallback when running from monorepo root
            paths.push(std::path::PathBuf::from(
                "code-graph/fixtures/xavier-plugins/plugins.json",
            ));
            paths
        };

        for path in candidates {
            match std::fs::read_to_string(&path) {
                Ok(raw) => match serde_json::from_str::<RegistryIndexFile>(&raw) {
                    Ok(index) => {
                        debug!(
                            path = %path.display(),
                            count = index.plugins.len(),
                            "loaded plugin registry index"
                        );
                        return index.plugins;
                    }
                    Err(err) => {
                        debug!(path = %path.display(), %err, "invalid plugins.json");
                    }
                },
                Err(_) => continue,
            }
        }
        debug!("no plugin registry index found; DefaultRegistry empty");
        Vec::new()
    }
}

impl PluginRegistry for DefaultRegistry {
    fn fetch_index(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<RegistryEntry>>> + Send>>
    {
        let entries = self.entries.clone();
        Box::pin(async move { Ok(entries) })
    }

    fn search(
        &self,
        query: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<RegistryEntry>>> + Send>>
    {
        let q = query.to_ascii_lowercase();
        let entries = self
            .entries
            .iter()
            .filter(|e| {
                e.name.to_ascii_lowercase().contains(&q)
                    || e.display_name.to_ascii_lowercase().contains(&q)
                    || e.description.to_ascii_lowercase().contains(&q)
            })
            .cloned()
            .collect::<Vec<_>>();
        Box::pin(async move { Ok(entries) })
    }

    fn get(
        &self,
        name: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<RegistryEntry>> + Send>> {
        let found = self.entries.iter().find(|e| e.name == name).cloned();
        let name = name.to_string();
        Box::pin(async move {
            found
                .ok_or_else(|| GraphError::Parser(format!("Plugin {} not found in registry", name)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn default_registry_loads_fixture_index() {
        let registry = DefaultRegistry::new();
        let index = registry.fetch_index().await.expect("fetch_index");
        assert!(
            !index.is_empty(),
            "fixture plugins.json must load into DefaultRegistry"
        );
        let py = registry
            .get("parser-python")
            .await
            .expect("parser-python in fixture");
        assert_eq!(py.name, "parser-python");
        let hits = registry.search("python").await.expect("search");
        assert!(hits.iter().any(|e| e.name == "parser-python"));
    }

    #[test]
    fn chain_prefers_installed_plugin_for_a_language() {
        let manager = PluginManager::new();
        manager.fallback().write().clear(&Language::Python);
        assert_eq!(
            manager.chain_for(&Language::Python),
            vec![FallbackStep::Native, FallbackStep::NoOp],
        );

        manager.register(PluginDescriptor {
            name: "parser-py".into(),
            version: "1.0.0".into(),
            command: "parser-py".into(),
            languages: vec![Language::Python],
            extensions: vec![],
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
            extensions: vec![],
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
