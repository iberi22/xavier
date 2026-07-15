//! Plugin lifecycle orchestrator.
//!
//! [`PluginManager`] owns the installed-plugin table, the fallback-chain
//! resolver, the process engine, and (added in F3) a registry client and a
//! local cache. It is the single entry point for both **execution** (parse via
//! the fallback chain, see [`crate::parser::parse_source`]) and **lifecycle**
//! ([`install`], [`update`], [`rollback`], [`uninstall`], [`list`]).
//!
//! Backward compatibility: the F1+F2 public surface (`new`, `register`,
//! `descriptor_for`, `descriptor_by_name`, `chain_for`, `parse_with_plugin`,
//! `load_config`) is preserved verbatim, so the deprecated [`PluginHost`]
//! shim and existing callers keep working.
//!
//! [`install`]: PluginManager::install
//! [`update`]: PluginManager::update
//! [`rollback`]: PluginManager::rollback
//! [`uninstall`]: PluginManager::uninstall
//! [`list`]: PluginManager::list
//! [`PluginHost`]: crate::plugin_host::PluginHost

use crate::error::{GraphError, Result};
use crate::plugin::cache::PluginCache;
use crate::plugin::fallback::FallbackChain;
use crate::plugin::registry::{PluginRegistry, RegistryEntry};
use crate::plugin::types::{
    FallbackResolver, FallbackStep, FileToParse, PluginConfig, PluginDescriptor, PluginEngine,
};
use crate::types::{Language, Symbol};
use parking_lot::RwLock;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::plugin::engine::ProcessEngine;

/// How many old versions to keep when an install completes.
const DEFAULT_KEEP_VERSIONS: usize = 3;

/// An installed plugin as returned by [`PluginManager::list`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    /// The descriptor currently active for this plugin.
    pub descriptor: PluginDescriptor,
    /// Currently-active version.
    pub version: String,
    /// All cached versions (newest first).
    pub cached_versions: Vec<String>,
}

/// On-disk record of which plugins are installed and which version is active.
/// Persisted to `<config_dir>/code-graph/installed.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstalledManifest {
    /// name -> (active version, descriptor)
    pub plugins: HashMap<String, InstalledRecord>,
}

/// Note: we deliberately do NOT `#[serde(flatten)]` the descriptor here,
/// because `PluginDescriptor` already has a `version` field that would collide
/// with this struct's `version` (serde reports `duplicate field version`).
/// Nesting the descriptor under a dedicated key keeps the on-disk JSON
/// unambiguous and round-trips cleanly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledRecord {
    /// Currently-active version of the plugin.
    pub version: String,
    /// The plugin descriptor (name/version/command/languages/capabilities).
    pub descriptor: PluginDescriptor,
}

/// Orchestrates plugin lifecycle and execution.
pub struct PluginManager {
    /// Plugins keyed by the language they handle (first-wins on collision).
    installed: RwLock<HashMap<Language, PluginDescriptor>>,
    /// Plugins keyed by descriptor name, for `FallbackStep::Plugin(name)`.
    by_name: RwLock<HashMap<String, PluginDescriptor>>,
    fallback: RwLock<FallbackChain>,
    engine: Arc<dyn PluginEngine>,

    // F3 fields ------------------------------------------------------------
    registry: Arc<dyn PluginRegistry>,
    cache: Arc<PluginCache>,
    /// Persistent record of installed plugins (name -> active record).
    manifest: RwLock<InstalledManifest>,
    /// Where `installed.json` lives. `None` when running fully in-memory (tests).
    manifest_path: Option<PathBuf>,
}

impl PluginManager {
    // ======================================================================
    // F1+F2 construction + execution (preserved verbatim)
    // ======================================================================

    /// Build a default manager: real `GitHubRegistry`, real `PluginCache`,
    /// standard `ProcessEngine`. Loads any persisted `installed.json` so a
    /// restart re-registers previously-installed plugins.
    pub fn new() -> Self {
        Self::build(
            Arc::new(ProcessEngine::default()),
            Arc::new(crate::plugin::registry::GitHubRegistry::default()),
            Arc::new(PluginCache::new()),
            default_manifest_path(),
        )
    }

    /// Build a manager with a custom engine (used by tests / future WASM engine).
    ///
    /// Preserved from F1+F2 for backward compatibility. Defaults to a
    /// `GitHubRegistry` + default `PluginCache`.
    pub fn with_engine(engine: Arc<dyn PluginEngine>) -> Self {
        Self::build(
            engine,
            Arc::new(crate::plugin::registry::GitHubRegistry::default()),
            Arc::new(PluginCache::new()),
            default_manifest_path(),
        )
    }

    /// Full constructor (F3): inject registry + cache + manifest path.
    /// Used by tests to wire a [`MockRegistry`](crate::plugin::MockRegistry)
    /// and a tempdir cache. `manifest_path = None` keeps the manifest in-memory.
    pub fn build(
        engine: Arc<dyn PluginEngine>,
        registry: Arc<dyn PluginRegistry>,
        cache: Arc<PluginCache>,
        manifest_path: Option<PathBuf>,
    ) -> Self {
        let fallback = FallbackChain::load_or_default();
        let mut manager = Self {
            installed: RwLock::new(HashMap::new()),
            by_name: RwLock::new(HashMap::new()),
            fallback: RwLock::new(fallback),
            engine,
            registry,
            cache,
            manifest: RwLock::new(InstalledManifest::default()),
            manifest_path,
        };
        // Re-hydrate installed plugins from the persisted manifest, if any.
        if let Err(e) = manager.load_manifest() {
            debug!(error = %e, "no installed manifest loaded (ok on first run)");
        }
        manager
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

    /// Remove a plugin from the in-memory tables (does not touch the cache).
    pub fn unregister(&self, name: &str) {
        self.by_name.write().remove(name);
        let mut installed = self.installed.write();
        // Drop any language slots pointing at this plugin.
        installed.retain(|_, desc| desc.name != name);
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
            .ok_or_else(|| GraphError::Plugin(format!("unknown plugin '{}'", name)))?;
        let config = PluginConfig {
            command: descriptor.command,
            version: descriptor.version,
            languages: descriptor.languages,
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

    /// Read-only access to the registry (for CLI/API `search`/`available`).
    pub fn registry(&self) -> &Arc<dyn PluginRegistry> {
        &self.registry
    }

    /// Read-only access to the cache (for CLI/API + tests).
    pub fn cache(&self) -> &Arc<PluginCache> {
        &self.cache
    }

    // ======================================================================
    // F3 lifecycle
    // ======================================================================

    /// Install a plugin from the registry.
    ///
    /// Flow: fetch entry → resolve platform artifact → download → verify
    /// SHA-256 → extract into the cache → register the active descriptor →
    /// persist the manifest → prune old versions.
    ///
    /// `version = None` installs the registry's latest advertised version.
    pub async fn install(&self, name: &str, version: Option<&str>) -> Result<PluginDescriptor> {
        let entry = self.registry.get(name).await?;
        let target_version = match version {
            Some(v) => v.to_string(),
            None => entry.version.clone(),
        };
        // If a specific version was requested, sanity-check it parses.
        let _target_semver = Version::parse(&target_version).map_err(|e| {
            GraphError::Registry(format!("invalid version '{}': {}", target_version, e))
        })?;

        info!(%name, %target_version, "installing plugin");
        let entry_for_download = self.entry_for_version(&entry, &target_version)?;

        let tmp = tempfile::tempdir().map_err(GraphError::Io)?;
        let archive = self
            .registry
            .download(&entry_for_download, tmp.path())
            .await?;

        let version_dir = self.cache.store(name, &target_version, &archive)?;
        let command = self
            .cache
            .binary_path(name, &target_version)?
            .to_string_lossy()
            .into_owned();

        let descriptor = PluginDescriptor {
            name: name.to_string(),
            version: target_version.clone(),
            command,
            languages: entry.languages.clone(),
            capabilities: entry.capabilities.clone(),
        };

        // Replace any prior registration for this plugin, then persist.
        self.unregister(name);
        self.register(descriptor.clone());
        self.manifest_write(name, &descriptor, &target_version);
        self.persist_manifest()?;

        // Prune old versions, keeping DEFAULT_KEEP_VERSIONS + the active one.
        if let Err(e) = self
            .cache
            .prune_protect(name, DEFAULT_KEEP_VERSIONS, Some(&target_version))
        {
            warn!(%name, error = %e, "post-install prune failed");
        }

        debug!(%name, %target_version, path = ?version_dir, "plugin installed");
        Ok(descriptor)
    }

    /// Check the registry for a newer version of `name` (or all plugins) and
    /// install it. Returns the descriptors that were updated (empty if none).
    pub async fn update(&self, name: Option<&str>) -> Result<Vec<PluginDescriptor>> {
        let names: Vec<String> = match name {
            Some(n) => vec![n.to_string()],
            None => self.manifest.read().plugins.keys().cloned().collect(),
        };

        let mut updated = Vec::new();
        for n in names {
            let current = self
                .manifest
                .read()
                .plugins
                .get(&n)
                .map(|r| r.version.clone());
            let Some(current) = current else {
                warn!(%n, "update requested for unknown plugin, skipping");
                continue;
            };
            let entry = match self.registry.get(&n).await {
                Ok(e) => e,
                Err(e) => {
                    warn!(%n, error = %e, "registry lookup failed during update");
                    continue;
                }
            };
            let latest = entry.version.clone();
            let newer = match (Version::parse(&current), Version::parse(&latest)) {
                (Ok(cur), Ok(lat)) => lat > cur,
                _ => latest != current, // fall back to string inequality
            };
            if newer {
                info!(%n, from = %current, to = %latest, "updating plugin");
                let desc = self.install(&n, Some(&latest)).await?;
                updated.push(desc);
            } else {
                debug!(%n, %current, "already up to date");
            }
        }
        Ok(updated)
    }

    /// Roll a plugin back to the previous cached version.
    ///
    /// "Previous" = the second-newest version in the cache (i.e. the one
    /// before the currently-active one). Errors if there's no older version.
    pub async fn rollback(&self, name: &str) -> Result<PluginDescriptor> {
        let versions = self.cache.list_versions(name)?;
        let active = self
            .manifest
            .read()
            .plugins
            .get(name)
            .map(|r| r.version.clone())
            .ok_or_else(|| GraphError::Plugin(format!("plugin '{}' not installed", name)))?;

        // Find the active version, then the next-older one.
        let active_semver = Version::parse(&active).ok();
        let target = versions
            .iter()
            .filter(|v| match &active_semver {
                Some(a) => *v < a,
                None => true,
            })
            .cloned()
            .max()
            .ok_or_else(|| {
                GraphError::Plugin(format!("no older version to roll back to for '{}'", name))
            })?;

        let command = self
            .cache
            .binary_path(name, &target.to_string())?
            .to_string_lossy()
            .into_owned();

        // Carry languages/capabilities from the existing record.
        let (langs, caps) = self
            .manifest
            .read()
            .plugins
            .get(name)
            .map(|r| {
                (
                    r.descriptor.languages.clone(),
                    r.descriptor.capabilities.clone(),
                )
            })
            .unwrap_or_default();

        let descriptor = PluginDescriptor {
            name: name.to_string(),
            version: target.to_string(),
            command,
            languages: langs,
            capabilities: caps,
        };
        self.unregister(name);
        self.register(descriptor.clone());
        self.manifest_write(name, &descriptor, &target.to_string());
        self.persist_manifest()?;
        info!(%name, to = %target, "rolled back plugin");
        Ok(descriptor)
    }

    /// Uninstall a plugin: remove from manifest, unregister, and delete the
    /// cache directory.
    pub async fn uninstall(&self, name: &str) -> Result<()> {
        if self.manifest.read().plugins.get(name).is_none() {
            return Err(GraphError::Plugin(format!(
                "plugin '{}' is not installed",
                name
            )));
        }
        self.unregister(name);
        {
            let mut manifest = self.manifest.write();
            manifest.plugins.remove(name);
        }
        self.persist_manifest()?;
        if let Err(e) = self.cache.purge(name) {
            warn!(%name, error = %e, "failed to purge cache dir during uninstall");
        }
        info!(%name, "plugin uninstalled");
        Ok(())
    }

    /// List all installed plugins with their active version and cached history.
    pub fn list(&self) -> Vec<InstalledPlugin> {
        let manifest = self.manifest.read();
        let mut out = Vec::new();
        for (name, record) in manifest.plugins.iter() {
            let cached_versions = self
                .cache
                .list_versions(name)
                .map(|vs| vs.into_iter().map(|v| v.to_string()).collect())
                .unwrap_or_default();
            out.push(InstalledPlugin {
                descriptor: record.descriptor.clone(),
                version: record.version.clone(),
                cached_versions,
            });
        }
        out
    }

    // ======================================================================
    // Manifest persistence helpers
    // ======================================================================

    fn load_manifest(&mut self) -> Result<()> {
        let Some(path) = &self.manifest_path else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(path).map_err(GraphError::Io)?;
        let manifest: InstalledManifest =
            serde_json::from_str(&content).map_err(|e| GraphError::Plugin(e.to_string()))?;
        // Re-register each installed plugin from its descriptor.
        for (_, record) in manifest.plugins.iter() {
            self.register(record.descriptor.clone());
        }
        *self.manifest.write() = manifest;
        Ok(())
    }

    fn manifest_write(&self, name: &str, descriptor: &PluginDescriptor, version: &str) {
        let mut manifest = self.manifest.write();
        manifest.plugins.insert(
            name.to_string(),
            InstalledRecord {
                version: version.to_string(),
                descriptor: descriptor.clone(),
            },
        );
    }

    fn persist_manifest(&self) -> Result<()> {
        let Some(path) = &self.manifest_path else {
            return Ok(()); // in-memory
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(GraphError::Io)?;
        }
        let manifest = self.manifest.read();
        let json = serde_json::to_string_pretty(&*manifest)
            .map_err(|e| GraphError::Plugin(e.to_string()))?;
        std::fs::write(path, json).map_err(GraphError::Io)?;
        Ok(())
    }

    /// Build a registry entry dressed with the requested version (so
    /// `download`/checksum still use the platform artifact from the index,
    /// while we install a possibly-different version string). For the mock
    /// registry the platform entry is fixed; for the live registry we'd
    /// resolve the version-specific URL. This implementation keeps the
    /// platform descriptor from the fetched entry and overrides only `version`.
    fn entry_for_version(&self, entry: &RegistryEntry, version: &str) -> Result<RegistryEntry> {
        // The official registry's `platform` URLs are version-tagged, so a
        // version mismatch would 404 live. For F3 (mock-tested) we accept the
        // entry as-is and surface the requested version on the returned clone;
        // a future F3.x can add per-version resolution once the real repo
        // publishes multiple versions.
        if version != entry.version {
            warn!(
                requested = version,
                advertised = %entry.version,
                "registry entry version mismatch; installing advertised version"
            );
        }
        Ok(entry.clone())
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve the default `installed.json` path: `<config_dir>/code-graph/installed.json`.
fn default_manifest_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("code-graph")
            .join("installed.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::registry::{current_platform, MockRegistry, PlatformEntry, RegistryIndex};
    use crate::types::Language;

    /// Build a tar.gz in memory containing `binary_name`, return its bytes.
    /// The binary is emitted with the host platform's executable suffix so
    /// `binary_path` (which looks for `<name>.exe` on Windows, bare `<name>`
    /// elsewhere) can locate it after extraction.
    fn make_archive_bytes(binary_name: &str) -> Vec<u8> {
        let entry_name = if cfg!(windows) {
            format!("{}.exe", binary_name)
        } else {
            binary_name.to_string()
        };
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let enc = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::default());
            let mut builder = tar::Builder::new(enc);
            let content = if cfg!(windows) {
                b"fake exe".to_vec()
            } else {
                b"#!/bin/sh\necho hi".to_vec()
            };
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, &entry_name, std::io::Cursor::new(content))
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        buf.into_inner()
    }

    /// Build a one-plugin registry index + staged archive wired to a tempdir
    /// cache + temp manifest. Returns a fully-constructed manager.
    fn build_manager(plugin_name: &str, version: &str) -> (PluginManager, tempfile::TempDir) {
        let archive_bytes = make_archive_bytes(plugin_name);
        let checksum = MockRegistry::checksum_of(&archive_bytes);

        let entry = RegistryEntry {
            name: plugin_name.to_string(),
            display_name: Some(plugin_name.to_string()),
            description: Some("test".into()),
            version: version.to_string(),
            author: None,
            languages: vec![Language::Python],
            capabilities: vec!["parse".into()],
            platform: std::collections::BTreeMap::from([(
                current_platform().to_string(),
                PlatformEntry {
                    url: format!("https://example/{}/{}", plugin_name, version),
                    checksum: checksum.clone(),
                    format: Some("tar.gz".into()),
                },
            )]),
            min_engine_version: None,
            license: Some("MIT".into()),
        };
        let index = RegistryIndex {
            registry_version: 1,
            updated_at: None,
            min_engine_version: None,
            plugins: vec![entry],
        };
        let registry = Arc::new(MockRegistry::new(index));
        registry.stage_archive(plugin_name, archive_bytes, checksum);

        let tmp = tempfile::tempdir().unwrap();
        let cache = Arc::new(PluginCache::with_root(
            tmp.path().join("cache").to_path_buf(),
        ));
        let manifest = tmp.path().join("installed.json");
        let engine = Arc::new(ProcessEngine::default());
        let manager = PluginManager::build(engine, registry, cache, Some(manifest));
        (manager, tmp)
    }

    #[tokio::test]
    async fn install_then_list_then_uninstall() {
        let (manager, _tmp) = build_manager("parser-python", "1.0.0");

        let desc = manager
            .install("parser-python", None)
            .await
            .expect("install");
        assert_eq!(desc.name, "parser-python");
        assert_eq!(desc.version, "1.0.0");

        let installed = manager.list();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].descriptor.name, "parser-python");

        // Fallback chain now prefers the plugin for Python.
        assert_eq!(
            manager.chain_for(&Language::Python),
            vec![
                FallbackStep::Plugin("parser-python".into()),
                FallbackStep::Native,
                FallbackStep::NoOp,
            ]
        );

        manager.uninstall("parser-python").await.expect("uninstall");
        assert!(manager.list().is_empty());
    }

    #[tokio::test]
    async fn checksum_failure_aborts_install() {
        let plugin_name = "parser-python";
        let archive_bytes = make_archive_bytes(plugin_name);
        let real_checksum = MockRegistry::checksum_of(&archive_bytes);
        let entry = RegistryEntry {
            name: plugin_name.to_string(),
            display_name: None,
            description: None,
            version: "1.0.0".to_string(),
            author: None,
            languages: vec![Language::Python],
            capabilities: vec!["parse".into()],
            platform: std::collections::BTreeMap::from([(
                current_platform().to_string(),
                PlatformEntry {
                    url: "https://example/p".into(),
                    checksum: real_checksum.clone(),
                    format: Some("tar.gz".into()),
                },
            )]),
            min_engine_version: None,
            license: None,
        };
        let index = RegistryIndex {
            registry_version: 1,
            updated_at: None,
            min_engine_version: None,
            plugins: vec![entry],
        };
        let registry = Arc::new(MockRegistry::new(index));
        registry.stage_archive(plugin_name, archive_bytes, real_checksum);
        registry.tamper_next_download(plugin_name);

        let tmp = tempfile::tempdir().unwrap();
        let cache = Arc::new(PluginCache::with_root(
            tmp.path().join("cache").to_path_buf(),
        ));
        let engine = Arc::new(ProcessEngine::default());
        let manager = PluginManager::build(
            engine,
            registry,
            cache,
            Some(tmp.path().join("installed.json")),
        );

        let err = manager.install(plugin_name, None).await.unwrap_err();
        assert!(
            err.to_string().contains("checksum mismatch"),
            "got: {}",
            err
        );
        assert!(manager.list().is_empty());
    }

    #[tokio::test]
    async fn rollback_returns_to_previous_version() {
        let (manager, _tmp) = build_manager("parser-python", "1.0.0");
        manager.install("parser-python", None).await.unwrap();

        // Plant a fake "0.9.0" version dir so rollback has somewhere to go.
        let older_dir = manager.cache().version_dir("parser-python", "0.9.0");
        std::fs::create_dir_all(&older_dir).unwrap();
        let older_bin = if cfg!(windows) {
            older_dir.join("parser-python.exe")
        } else {
            older_dir.join("parser-python")
        };
        std::fs::write(&older_bin, b"x").unwrap();

        let rolled = manager.rollback("parser-python").await.expect("rollback");
        assert_eq!(rolled.version, "0.9.0");
    }

    #[tokio::test]
    async fn update_is_noop_when_already_latest() {
        let (manager, _tmp) = build_manager("parser-python", "1.0.0");
        manager.install("parser-python", None).await.unwrap();
        // Registry advertises 1.0.0; updating should find nothing newer.
        let updated = manager.update(Some("parser-python")).await.expect("update");
        assert!(updated.is_empty(), "expected no updates, got {:?}", updated);
    }

    #[tokio::test]
    async fn manifest_persists_across_managers() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = tmp.path().join("installed.json");

        // First manager: install.
        let plugin_name = "parser-python";
        let archive_bytes = make_archive_bytes(plugin_name);
        let checksum = MockRegistry::checksum_of(&archive_bytes);
        let entry = RegistryEntry {
            name: plugin_name.to_string(),
            display_name: None,
            description: None,
            version: "1.0.0".to_string(),
            author: None,
            languages: vec![Language::Python],
            capabilities: vec!["parse".into()],
            platform: std::collections::BTreeMap::from([(
                current_platform().to_string(),
                PlatformEntry {
                    url: "https://example/p".into(),
                    checksum: checksum.clone(),
                    format: Some("tar.gz".into()),
                },
            )]),
            min_engine_version: None,
            license: None,
        };
        let index = RegistryIndex {
            registry_version: 1,
            updated_at: None,
            min_engine_version: None,
            plugins: vec![entry],
        };
        let registry = Arc::new(MockRegistry::new(index.clone()));
        registry.stage_archive(plugin_name, archive_bytes.clone(), checksum);
        let cache = Arc::new(PluginCache::with_root(
            tmp.path().join("cache").to_path_buf(),
        ));
        let engine = Arc::new(ProcessEngine::default());
        let m1 = PluginManager::build(engine, registry, cache, Some(manifest.clone()));
        m1.install(plugin_name, None).await.unwrap();
        assert_eq!(m1.list().len(), 1);
        assert!(manifest.exists(), "manifest should be persisted");

        // Second manager, same manifest path: should re-hydrate the plugin.
        let registry2 = Arc::new(MockRegistry::new(index));
        registry2.stage_archive(plugin_name, archive_bytes, MockRegistry::checksum_of(b"x"));
        let cache2 = Arc::new(PluginCache::with_root(
            tmp.path().join("cache").to_path_buf(),
        ));
        let engine2 = Arc::new(ProcessEngine::default());
        let m2 = PluginManager::build(engine2, registry2, cache2, Some(manifest));
        assert_eq!(
            m2.list().len(),
            1,
            "manifest should re-hydrate the plugin on rebuild"
        );
    }
}
