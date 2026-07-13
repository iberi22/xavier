# Issue: Plugin System — CLI + Health + API

**Prerequisite: F1+F2 COMPLETE ✅** (Commit bf2dec28)
- `Language::Other(String)` implemented
- `PluginManager`, `ProcessEngine`, `FallbackChain` exist in `code-graph/src/plugin/`
- `parse_source()` uses fallback chain with Plugin→Native→NoOp
- `plugin_host.rs` deprecated as wrapper

## Labels
`enhancement`, `plugin-system`, `P4-cli`

## Changes Required

### 1. `code-graph/src/commands/plugin_cmds.rs` — New file
CLI commands for plugin management:

```
code-graph plugin list                           # List installed plugins
code-graph plugin search <query>                 # Search registry
code-graph plugin install <name> [--version]     # Install plugin
code-graph plugin update [name]                  # Update all or one
code-graph plugin rollback <name>                # Rollback to previous
code-graph plugin uninstall <name>               # Uninstall
code-graph plugin health [name]                  # Health check
```

### 2. `code-graph/src/plugin/health.rs` — New file
Health monitoring with ring buffer:

```rust
pub struct PluginHealthMonitor {
    states: RwLock<HashMap<String, PluginHealth>>,
    metrics: RwLock<HashMap<String, MetricsRingBuffer>>,
    check_interval: Duration,
}

pub struct MetricsRingBuffer {
    entries: VecDeque<MetricsEntry>,
    max_entries: usize,  // 1000
}

impl PluginHealthMonitor {
    pub fn new(check_interval: Duration) -> Self;
    
    /// Run health checks on all plugins
    pub async fn check_all(&self, manager: &PluginManager) -> Vec<PluginHealth>;
    
    /// Check a single plugin
    pub async fn check_one(&self, mgr: &PluginManager, name: &str) -> Result<PluginHealth>;
    
    /// Get metrics for a plugin
    pub fn metrics(&self, name: &str) -> Result<PluginMetrics>;
    
    /// Record a health event
    pub fn record(&self, name: &str, latency_ms: u64, success: bool, error: Option<String>);
    
    /// Start background health checks (60s interval)
    pub fn start_background_check(self: Arc<Self>, manager: Arc<PluginManager>);
}
```

### 3. `code-graph/src/plugin/discovery.rs` — New file
Plugin language discovery:

```rust
pub struct LanguageDiscovery {
    manager: Arc<PluginManager>,
}

impl LanguageDiscovery {
    /// Build language→plugin mapping from installed plugins
    pub fn discover(&self) -> HashMap<String, Vec<String>>;
    
    /// Get all supported languages
    pub fn languages(&self) -> Vec<Language>;
    
    /// Find plugins for a specific language
    pub fn plugins_for(&self, lang: &str) -> Vec<PluginDescriptor>;
}
```

### 4. `code-graph/src/api/plugin_routes.rs` — New file
```
GET    /api/v1/plugins                         → list installed
GET    /api/v1/plugins/available               → list available (registry)
GET    /api/v1/plugins/health                  → aggregate health
POST   /api/v1/plugins/install                 → install {name, version?}
POST   /api/v1/plugins/:name/update            → update
POST   /api/v1/plugins/:name/rollback          → rollback
DELETE /api/v1/plugins/:name                   → uninstall
GET    /api/v1/plugins/:name/health            → single plugin health
GET    /api/v1/plugins/:name/metrics           → detailed metrics
GET    /api/v1/plugins/fallback                → all fallback chains
POST   /api/v1/plugins/fallback/:lang          → set chain
GET    /api/v1/languages                       → discovered languages
```

### 5. `code-graph/src/main.rs` — Add Plugin subcommand
```rust
#[command(subcommand)]
enum Commands { ... Plugin(PluginCommands), ... }
```

## Definition of Done
- [ ] All CLI commands functional: list, search, install, update, rollback, uninstall, health
- [ ] Plugin health monitor records metrics in ring buffer
- [ ] Background health check at 60s interval
- [ ] Language discovery builds mapping from installed plugins
- [ ] All API endpoints return correct JSON responses
- [ ] `cargo build --release` passes
- [ ] Integration tests for CLI commands
