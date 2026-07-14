//! Plugin registry client.
//!
//! A registry is the source of truth for *available* plugins (as opposed to
//! installed ones, tracked by [`crate::plugin::PluginManager`]). The canonical
//! implementation [`GitHubRegistry`] fetches a JSON index from
//! `https://raw.githubusercontent.com/swal/xavier-plugins/main/plugins.json`
//! and serves plugin archives from GitHub Releases.
//!
//! The registry is abstracted behind the [`PluginRegistry`] trait so that
//! tests and offline environments can drive the full lifecycle via
//! [`MockRegistry`] without touching the network. The real `swal/xavier-plugins`
//! repo is tracked separately (see `.gitcore/issues/issue-plugin-system-registry-repo.md`);
//! until it exists, [`GitHubRegistry`] is wired but its live path is exercised
//! only by the `#[ignore]`d network test.

use crate::error::{GraphError, Result};
use crate::types::Language;
use parking_lot::RwLock;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// How long a fetched index stays fresh before being refetched.
pub const INDEX_TTL: Duration = Duration::from_secs(5 * 60);

/// Canonical registry URL for the official `swal/xavier-plugins` index.
pub const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/swal/xavier-plugins/main/plugins.json";

/// User-Agent sent on registry HTTP requests (GitHub raw blocks empty UAs).
const USER_AGENT: &str = concat!("code-graph/", env!("CARGO_PKG_VERSION"));

// ============================================================================
// Index schema (matches FEATURE-plugin-system.md § Plugin Registry Schema)
// ============================================================================

/// Top-level `plugins.json` document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryIndex {
    pub registry_version: u32,
    #[serde(default)]
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub min_engine_version: Option<String>,
    pub plugins: Vec<RegistryEntry>,
}

/// A single plugin advertised in the registry index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Latest version advertised by the registry (semver).
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    pub languages: Vec<Language>,
    pub capabilities: Vec<String>,
    /// Per-platform download descriptors.
    #[serde(default)]
    pub platform: std::collections::BTreeMap<String, PlatformEntry>,
    #[serde(default)]
    pub min_engine_version: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
}

impl RegistryEntry {
    pub fn parsed_version(&self) -> Result<Version> {
        Version::parse(&self.version)
            .map_err(|e| GraphError::Registry(format!("invalid version '{}': {}", self.version, e)))
    }
}

/// Platform-specific artifact for a registry entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformEntry {
    pub url: String,
    /// `sha256:<hex>` checksum used by [`PluginRegistry::verify_checksum`].
    pub checksum: String,
    /// `tar.gz` | `zip` | `wasm`.
    #[serde(default)]
    pub format: Option<String>,
}

/// Detect the current platform key (matches the registry's `platform` map).
pub fn current_platform() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "linux-arm64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "macos"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "macos-arm64"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows"
    }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
    )))]
    {
        "unknown"
    }
}

// ============================================================================
// Registry trait
// ============================================================================

/// Source of available plugins.
///
/// Implemented by [`GitHubRegistry`] (live) and [`MockRegistry`] (tests/offline).
/// Methods that touch the network are `async`; pure helpers (`verify_checksum`)
/// are synchronous.
#[async_trait::async_trait]
pub trait PluginRegistry: Send + Sync {
    /// Fetch (or serve from cache) the full plugin index.
    async fn fetch_index(&self) -> Result<Arc<RegistryIndex>>;

    /// Case-insensitive substring search across name/display_name/description.
    async fn search(&self, query: &str) -> Result<Vec<RegistryEntry>> {
        let index = self.fetch_index().await?;
        let q = query.to_lowercase();
        Ok(index
            .plugins
            .iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&q)
                    || e.display_name
                        .as_deref()
                        .is_some_and(|s| s.to_lowercase().contains(&q))
                    || e.description
                        .as_deref()
                        .is_some_and(|s| s.to_lowercase().contains(&q))
            })
            .cloned()
            .collect())
    }

    /// Look up a single entry by exact name.
    async fn get(&self, name: &str) -> Result<RegistryEntry> {
        let index = self.fetch_index().await?;
        index
            .plugins
            .iter()
            .find(|e| e.name == name)
            .cloned()
            .ok_or_else(|| GraphError::Registry(format!("plugin '{}' not found in registry", name)))
    }

    /// Resolve the platform artifact for the *current* platform, erroring if
    /// the entry doesn't ship one for us.
    async fn platform_for_current(&self, entry: &RegistryEntry) -> Result<PlatformEntry> {
        let key = current_platform();
        entry.platform.get(key).cloned().ok_or_else(|| {
            GraphError::Registry(format!(
                "plugin '{}' has no artifact for platform '{}'",
                entry.name, key
            ))
        })
    }

    /// Download `entry`'s current-platform archive into `dest` and verify its
    /// checksum. Returns the path to the downloaded archive.
    async fn download(&self, entry: &RegistryEntry, dest: &Path) -> Result<PathBuf>;

    /// SHA-256 check. `expected` follows the registry's `sha256:<hex>` format;
    /// a bare hex digest is also accepted. Returns `Ok(())` on match.
    fn verify_checksum(&self, file: &Path, expected: &str) -> Result<()> {
        let expected_hex = expected
            .strip_prefix("sha256:")
            .unwrap_or(expected)
            .to_ascii_lowercase();
        let mut hasher = Sha256::new();
        let bytes = std::fs::read(file).map_err(GraphError::Io)?;
        hasher.update(&bytes);
        let actual = format!("{:x}", hasher.finalize());
        if actual == expected_hex {
            Ok(())
        } else {
            Err(GraphError::Plugin(format!(
                "checksum mismatch for {}: expected {}, got {}",
                file.display(),
                expected_hex,
                actual
            )))
        }
    }
}

// ============================================================================
// GitHubRegistry (live)
// ============================================================================

/// Live registry backed by the `swal/xavier-plugins` GitHub repo.
pub struct GitHubRegistry {
    registry_url: String,
    client: reqwest::Client,
    cache: RwLock<Option<(Arc<RegistryIndex>, Instant)>>,
    ttl: Duration,
}

impl Default for GitHubRegistry {
    fn default() -> Self {
        Self::new(DEFAULT_REGISTRY_URL.to_string(), INDEX_TTL)
    }
}

impl GitHubRegistry {
    pub fn new(registry_url: String, ttl: Duration) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            registry_url,
            client,
            cache: RwLock::new(None),
            ttl,
        }
    }

    /// Force-clear the in-memory index cache (next `fetch_index` hits network).
    pub fn invalidate(&self) {
        *self.cache.write() = None;
    }
}

#[async_trait::async_trait]
impl PluginRegistry for GitHubRegistry {
    async fn fetch_index(&self) -> Result<Arc<RegistryIndex>> {
        // Serve from cache if fresh.
        if let Some((idx, fetched_at)) = self.cache.read().as_ref() {
            if fetched_at.elapsed() < self.ttl {
                return Ok(idx.clone());
            }
        }

        debug!(url = %self.registry_url, "fetching plugin registry index");
        let resp = self
            .client
            .get(&self.registry_url)
            .send()
            .await
            .map_err(|e| GraphError::Registry(format!("registry fetch failed: {}", e)))?;
        if !resp.status().is_success() {
            return Err(GraphError::Registry(format!(
                "registry returned HTTP {}",
                resp.status()
            )));
        }
        let index: RegistryIndex = resp
            .json()
            .await
            .map_err(|e| GraphError::Registry(format!("registry index parse failed: {}", e)))?;

        let arc = Arc::new(index);
        *self.cache.write() = Some((arc.clone(), Instant::now()));
        Ok(arc)
    }

    async fn download(&self, entry: &RegistryEntry, dest: &Path) -> Result<PathBuf> {
        let platform = self.platform_for_current(entry).await?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(GraphError::Io)?;
        }
        let archive_path = dest.join(format!(
            "{}-{}{}",
            entry.name,
            entry.version,
            archive_extension(platform.format.as_deref())
        ));

        debug!(url = %platform.url, dest = %archive_path.display(), "downloading plugin");
        let resp = self
            .client
            .get(&platform.url)
            .send()
            .await
            .map_err(|e| GraphError::Registry(format!("download failed: {}", e)))?;
        if !resp.status().is_success() {
            return Err(GraphError::Registry(format!(
                "download returned HTTP {} for {}",
                resp.status(),
                platform.url
            )));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| GraphError::Registry(format!("download body failed: {}", e)))?;
        std::fs::write(&archive_path, &bytes).map_err(GraphError::Io)?;

        self.verify_checksum(&archive_path, &platform.checksum)?;
        Ok(archive_path)
    }
}

/// Pick the archive extension for a declared format.
fn archive_extension(format: Option<&str>) -> &'static str {
    match format {
        Some("zip") => ".zip",
        Some("wasm") => ".wasm",
        // default to tar.gz
        _ => ".tar.gz",
    }
}

// ============================================================================
// MockRegistry (tests / offline)
// ============================================================================

/// In-memory registry serving an injected index. `download` copies a
/// pre-staged archive from `archives[name]` into `dest`, so tests can exercise
/// the full install → checksum → extract path with zero network.
pub struct MockRegistry {
    index: Arc<RegistryIndex>,
    /// `name -> (bytes, declared_checksum)` archives served by `download`.
    archives: RwLock<std::collections::HashMap<String, (Vec<u8>, String)>>,
    /// When true, `download` deliberately writes a tampered payload so the
    /// checksum check fails — used to test the mismatch path.
    tamper: RwLock<std::collections::HashSet<String>>,
}

impl MockRegistry {
    pub fn new(index: RegistryIndex) -> Self {
        Self {
            index: Arc::new(index),
            archives: RwLock::new(Default::default()),
            tamper: RwLock::new(Default::default()),
        }
    }

    /// Register an archive to be served for `name`. `checksum` should be
    /// `sha256:<hex>` matching `bytes` (use [`MockRegistry::checksum_of`]).
    pub fn stage_archive(&self, name: &str, bytes: Vec<u8>, checksum: String) {
        self.archives
            .write()
            .insert(name.to_string(), (bytes, checksum));
    }

    /// Force the next `download` of `name` to fail its checksum check.
    pub fn tamper_next_download(&self, name: &str) {
        self.tamper.write().insert(name.to_string());
    }

    /// Compute the `sha256:<hex>` checksum of `bytes` for staging.
    pub fn checksum_of(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("sha256:{:x}", hasher.finalize())
    }

    /// Build a minimal valid index with the given entries.
    pub fn index_from_entries(entries: Vec<RegistryEntry>) -> RegistryIndex {
        RegistryIndex {
            registry_version: 1,
            updated_at: Some(chrono::Utc::now()),
            min_engine_version: None,
            plugins: entries,
        }
    }
}

#[async_trait::async_trait]
impl PluginRegistry for MockRegistry {
    async fn fetch_index(&self) -> Result<Arc<RegistryIndex>> {
        Ok(self.index.clone())
    }

    async fn download(&self, entry: &RegistryEntry, dest: &Path) -> Result<PathBuf> {
        let (bytes, checksum) =
            self.archives
                .read()
                .get(&entry.name)
                .cloned()
                .ok_or_else(|| {
                    GraphError::Registry(format!(
                        "mock registry has no staged archive for '{}'",
                        entry.name
                    ))
                })?;

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(GraphError::Io)?;
        }
        let archive_path = dest.join(format!(
            "{}-{}{}",
            entry.name,
            entry.version,
            archive_extension(
                entry
                    .platform
                    .get(current_platform())
                    .and_then(|p| p.format.as_deref()),
            )
        ));

        let to_write: Vec<u8> = if self.tamper.write().remove(&entry.name) {
            // Flip the last byte so the checksum no longer matches.
            let mut corrupted = bytes.clone();
            if let Some(last) = corrupted.last_mut() {
                *last ^= 0xFF;
            }
            warn!(name = %entry.name, "mock registry tampering with download");
            corrupted
        } else {
            bytes
        };

        std::fs::write(&archive_path, &to_write).map_err(GraphError::Io)?;
        self.verify_checksum(&archive_path, &checksum)?;
        Ok(archive_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(name: &str, version: &str) -> RegistryEntry {
        RegistryEntry {
            name: name.to_string(),
            display_name: Some(name.to_string()),
            description: Some("test".into()),
            version: version.to_string(),
            author: None,
            languages: vec![Language::Python],
            capabilities: vec!["parse".into()],
            platform: std::collections::BTreeMap::from([(
                current_platform().to_string(),
                PlatformEntry {
                    url: format!("https://example/{}/{}", name, version),
                    checksum: "PLACEHOLDER".into(),
                    format: Some("tar.gz".into()),
                },
            )]),
            min_engine_version: None,
            license: Some("MIT".into()),
        }
    }

    #[tokio::test]
    async fn mock_index_search_get() {
        let index = MockRegistry::index_from_entries(vec![
            sample_entry("parser-python", "1.0.0"),
            sample_entry("parser-ruby", "0.2.0"),
        ]);
        let reg = MockRegistry::new(index);

        let found = reg.search("ruby").await.expect("search");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "parser-ruby");

        let got = reg.get("parser-python").await.expect("get");
        assert_eq!(got.parsed_version().unwrap(), Version::new(1, 0, 0));

        assert!(reg.get("missing").await.is_err());
    }

    #[tokio::test]
    async fn mock_download_verifies_checksum() {
        let payload = b"hello plugin";
        let checksum = MockRegistry::checksum_of(payload);
        let mut entry = sample_entry("parser-python", "1.0.0");
        entry.platform.get_mut(current_platform()).unwrap().checksum = checksum.clone();

        let reg = MockRegistry::new(MockRegistry::index_from_entries(vec![entry.clone()]));
        reg.stage_archive("parser-python", payload.to_vec(), checksum);

        let dir = tempfile::tempdir().unwrap();
        let path = reg
            .download(&entry, dir.path())
            .await
            .expect("download + checksum");
        assert!(path.exists());
    }

    #[tokio::test]
    async fn mock_download_tamper_fails_checksum() {
        let payload = b"hello plugin";
        let checksum = MockRegistry::checksum_of(payload);
        let mut entry = sample_entry("parser-python", "1.0.0");
        entry.platform.get_mut(current_platform()).unwrap().checksum = checksum.clone();

        let reg = MockRegistry::new(MockRegistry::index_from_entries(vec![entry.clone()]));
        reg.stage_archive("parser-python", payload.to_vec(), checksum);
        reg.tamper_next_download("parser-python");

        let dir = tempfile::tempdir().unwrap();
        let result = reg.download(&entry, dir.path()).await;
        assert!(result.is_err(), "tampered download must fail checksum");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("checksum mismatch"), "got: {}", err);
    }

    #[test]
    fn verify_checksum_accepts_bare_hex_and_prefixed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.bin");
        std::fs::write(&file, b"abc").unwrap();
        let hex = {
            let mut h = Sha256::new();
            h.update(b"abc");
            format!("{:x}", h.finalize())
        };
        let reg = MockRegistry::new(MockRegistry::index_from_entries(vec![]));
        assert!(reg
            .verify_checksum(&file, &format!("sha256:{}", hex))
            .is_ok());
        assert!(reg.verify_checksum(&file, &hex).is_ok());
        assert!(reg.verify_checksum(&file, "sha256:deadbeef").is_err());
    }

    #[tokio::test]
    #[ignore = "hits live network; un-skip once swal/xavier-plugins exists"]
    async fn github_registry_fetch_live() {
        let reg = GitHubRegistry::default();
        let _ = reg.fetch_index().await; // ok to fail; just exercises the path
    }
}
