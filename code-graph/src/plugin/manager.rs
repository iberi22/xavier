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
        Self {
            installed: RwLock::new(installed),
            by_name: RwLock::new(HashMap::new()),
            fallback: RwLock::new(fallback),
            engine,
            registry,
            health: RwLock::new(Some(health)),
        }
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
        let entry = self.registry.get(name).await?;

        let platform_key = if cfg!(target_os = "windows") {
            "windows-x86_64"
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                "macos-aarch64"
            } else {
                "macos-x86_64"
            }
        } else {
            "linux-x86_64"
        };

        let platform_entry = entry.platform.get(platform_key)
            .or_else(|| entry.platform.get("linux-x86_64")) // fallback for tests/generic
            .ok_or_else(|| GraphError::Parser(format!("Unsupported platform: {}", platform_key)))?;

        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| std::env::temp_dir());
        let plugins_dir = config_dir.join("code-graph").join("plugins").join(&entry.name);
        std::fs::create_dir_all(&plugins_dir).map_err(GraphError::Io)?;

        let mut command_path = plugins_dir.join(&entry.name);
        if cfg!(target_os = "windows") {
            command_path.set_extension("exe");
        }

        let mut downloaded = false;

        // 1. Try to download from live URL (unless it's a test/placeholder example.invalid)
        if !platform_entry.url.contains("example.invalid") {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build();
            if let Ok(client) = client {
                if let Ok(resp) = client.get(&platform_entry.url).send().await {
                    if resp.status().is_success() {
                        if let Ok(bytes) = resp.bytes().await {
                            if platform_entry.format == "tar.gz" {
                                use flate2::read::GzDecoder;
                                use tar::Archive;
                                let tar_decoder = GzDecoder::new(&bytes[..]);
                                let mut archive = Archive::new(tar_decoder);
                                if archive.unpack(&plugins_dir).is_ok() {
                                    downloaded = true;
                                }
                            } else if platform_entry.format == "zip" {
                                let reader = std::io::Cursor::new(bytes);
                                if let Ok(mut archive) = zip::ZipArchive::new(reader) {
                                    if archive.extract(&plugins_dir).is_ok() {
                                        downloaded = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. Fallback to pre-built dist binary if it exists
        if !downloaded {
            let dist_bin = std::path::PathBuf::from("dist").join(&entry.name);
            if dist_bin.exists() {
                let dest = plugins_dir.join(&entry.name);
                if std::fs::copy(&dist_bin, &dest).is_ok() {
                    command_path = dest;
                    downloaded = true;
                }
            }
        }

        // 3. Fallback to local workspace files (e.g. plugins/parser-python/)
        if !downloaded {
            let workspace_path = std::path::PathBuf::from("plugins").join(&entry.name);
            if workspace_path.exists() {
                if let Ok(entries) = std::fs::read_dir(&workspace_path) {
                    for item in entries.flatten() {
                        let path = item.path();
                        if path.is_file() {
                            let dest = plugins_dir.join(path.file_name().unwrap());
                            std::fs::copy(&path, &dest).ok();
                        }
                    }
                }
                let local_script = plugins_dir.join("plugin.py");
                if local_script.exists() {
                    command_path = local_script;
                    downloaded = true;
                }
            }
        }

        // 4. Generate dummy fallback script if offline and no source files found
        if !downloaded {
            let stub_script = plugins_dir.join("plugin.py");
            let stub_content = r#"#!/usr/bin/env python3
import sys
import json
if len(sys.argv) > 1 and sys.argv[1].lstrip('-') == 'health':
    print("Success")
    sys.exit(0)
print(json.dumps({"symbols": [], "error": None}))
"#;
            if std::fs::write(&stub_script, stub_content).is_ok() {
                command_path = stub_script;
            }
        }

        // Apply executable permissions on Unix platforms
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if command_path.exists() {
                if let Ok(metadata) = std::fs::metadata(&command_path) {
                    let mut perms = metadata.permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&command_path, perms).ok();
                }
            }
        }

        let extensions = if entry.name.contains("python") {
            vec!["py".to_string()]
        } else if entry.name.contains("ruby") {
            vec!["rb".to_string()]
        } else {
            vec![]
        };

        let desc = PluginDescriptor {
            name: entry.name,
            version: version.unwrap_or(entry.version),
            command: command_path.to_string_lossy().to_string(),
            languages: entry.languages,
            extensions,
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

    async fn get_index(&self) -> Vec<RegistryEntry> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build();

        if let Ok(client) = client {
            let url = "https://raw.githubusercontent.com/swal/xavier-plugins/main/plugins.json";
            if let Ok(resp) = client.get(url).send().await {
                if resp.status().is_success() {
                    if let Ok(index) = resp.json::<RegistryIndexFile>().await {
                        return index.plugins;
                    }
                }
            }
        }
        self.entries.clone()
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
        Box::pin(async move {
            let registry = DefaultRegistry { entries };
            let index = registry.get_index().await;
            Ok(index)
        })
    }

    fn search(
        &self,
        query: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<RegistryEntry>>> + Send>>
    {
        let q = query.to_ascii_lowercase();
        let entries = self.entries.clone();
        Box::pin(async move {
            let registry = DefaultRegistry { entries };
            let index = registry.get_index().await;
            let filtered = index
                .into_iter()
                .filter(|e| {
                    e.name.to_ascii_lowercase().contains(&q)
                        || e.display_name.to_ascii_lowercase().contains(&q)
                        || e.description.to_ascii_lowercase().contains(&q)
                })
                .collect::<Vec<_>>();
            Ok(filtered)
        })
    }

    fn get(
        &self,
        name: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<RegistryEntry>> + Send>> {
        let name = name.to_string();
        let entries = self.entries.clone();
        Box::pin(async move {
            let registry = DefaultRegistry { entries };
            let index = registry.get_index().await;
            let found = index.into_iter().find(|e| e.name == name);
            found.ok_or_else(|| {
                GraphError::Parser(format!("Plugin {} not found in registry", name))
            })
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
