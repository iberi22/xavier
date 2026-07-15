# Issue: Plugin System — Registry + Lifecycle

**Prerequisite: F1+F2 COMPLETE ✅** (Commit bf2dec28)
- `Language::Other(String)` implemented
- `PluginManager`, `ProcessEngine`, `FallbackChain` exist in `code-graph/src/plugin/`
- `parse_source()` uses fallback chain
- `plugin_host.rs` deprecated as wrapper

## Labels
`enhancement`, `plugin-system`, `P3-registry`

## Changes Required

### 1. `code-graph/src/plugin/registry.rs` — New file
GitHub-based plugin registry client:

```rust
pub struct GitHubRegistry {
    registry_url: String,
    local_cache: PathBuf,
    index_cache: RwLock<Vec<RegistryEntry>>,
}

impl GitHubRegistry {
    pub fn new() -> Self;
    
    /// Fetch plugin index from GitHub raw URL
    pub async fn fetch_index(&self) -> Result<Vec<RegistryEntry>>;
    
    /// Search available plugins
    pub async fn search(&self, query: &str) -> Result<Vec<RegistryEntry>>;
    
    /// Get specific plugin entry
    pub async fn get(&self, name: &str) -> Result<RegistryEntry>;
    
    /// Download plugin archive + verify checksum
    pub async fn download(&self, entry: &PlatformEntry, dest: &Path) -> Result<PathBuf>;
    
    /// SHA-256 verification
    pub fn verify_checksum(&self, file: &Path, expected: &str) -> bool;
}
```

Registry URL: `https://raw.githubusercontent.com/swal/xavier-plugins/main/plugins.json`

### 2. `code-graph/src/plugin/cache.rs` — New file
Local plugin cache manager:

```rust
pub struct PluginCache {
    base_dir: PathBuf,  // ~/.xavier/plugins/
}

impl PluginCache {
    /// Store a downloaded plugin
    pub fn store(&self, name: &str, version: &str, archive: &Path) -> Result<PathBuf>;
    
    /// Get plugin binary path
    pub fn binary_path(&self, name: &str, version: &str) -> PathBuf;
    
    /// List cached versions of a plugin
    pub fn list_versions(&self, name: &str) -> Result<Vec<String>>;
    
    /// Remove old versions (keep N latest)
    pub fn prune(&self, name: &str, keep: usize) -> Result<()>;
    
    /// Extract archive (tar.gz/zip/wasm)
    pub fn extract(&self, archive: &Path, dest: &Path) -> Result<()>;
}
```

### 3. `code-graph/src/plugin/manager.rs` — Lifecycle methods
Add to PluginManager:

```rust
impl PluginManager {
    pub async fn install(&self, name: &str, version: Option<&str>) -> Result<PluginDescriptor>;
    pub async fn update(&self, name: Option<&str>) -> Result<Vec<PluginDescriptor>>;
    pub async fn rollback(&self, name: &str) -> Result<PluginDescriptor>;
    pub async fn uninstall(&self, name: &str) -> Result<()>;
    pub fn list(&self) -> Vec<PluginDescriptor>;
}
```

### 4. Cargo.toml — Add dependencies
```toml
reqwest = { version = "0.12", features = ["rustls-tls"] }
sha2 = "0.10"  # already present
flate2 = "1"
tar = "0.4"
zip = "2"
semver = "1"
chrono = "0.4"
```

## Definition of Done
- [ ] `registry.rs` fetches plugin index from GitHub
- [ ] `registry.rs` verifies SHA-256 checksum after download
- [ ] `cache.rs` stores and extracts plugins correctly
- [ ] `install()` downloads, verifies, extracts, and registers
- [ ] `update()` checks for newer version
- [ ] `rollback()` reverts to previous cached version
- [ ] `uninstall()` removes plugin files
- [ ] `cargo build --release` passes
- [ ] Tests cover: install, update, rollback, uninstall, checksum failure
