# FEATURE: Plugin System for Code-Graph & Xavier

**Feature ID:** `feat-plugin-system`
**Category:** `infrastructure`
**Status:** `planned`
**Progress:** 0%
**Version Target:** v0.12.0
**Issue:** #TBD

---

## 🎯 Objective

Extract all language-specific parsing logic from Xavier's code-graph into **downloadable, externally-managed plugins**. This reduces Xavier's core complexity, isolates failure domains, enables graceful fallback chains, and allows community-contributed language parsers without recompiling Xavier.

## 📊 Current State

- `code-graph/src/plugin_host.rs` — Exists with basic PluginHost, PluginConfig, PluginRequest/PluginResponse, stdin/stdout subprocess execution
- `code-graph/src/parser/mod.rs` — `parse_source()` with hardcoded dispatch to 7 native tree-sitter parsers
- `code-graph/src/types.rs` — `Language` enum with 8 hardcoded variants (Rust, TS, JS, Python, Go, Java, C, Cpp, Unknown)
- **Bug found**: Rust parser is hardcoded Native-only (line 91-93 of plugin_host.rs). Duplicate Python/TypeScript match arms in parser/mod.rs (lines 82-89)
- **Bug found**: SQLite on NTFS/WSL causes "disk I/O error" — DB moved to ext4 in v3.0 fix

## 🏗️ Architectural Design

### High-Level Architecture

```
┌──────────────────────────────────────────────────────────┐
│                   Xavier Core                            │
│  ┌──────────────────────────────────────────────────┐   │
│  │           Plugin Manager (manager.rs)             │   │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────────────┐  │   │
│  │  │ Registry  │ │  Engine  │ │  Fallback Chain  │  │   │
│  │  │ (GitHub)  │ │ (spawn)  │ │  (per language)  │  │   │
│  │  └────┬─────┘ └────┬─────┘ └────────┬─────────┘  │   │
│  └───────┼────────────┼────────────────┼─────────────┘   │
│          │            │                │                  │
│  ┌───────▼────────────▼────────────────▼──────────────┐  │
│  │              Plugin Discovery Layer                │  │
│  │  Scans ~/.xavier/plugins/ → builds language→plugin │  │
│  └───────────────────────┬───────────────────────────┘  │
│                          │                               │
│  ┌───────────────────────▼───────────────────────────┐  │
│  │            Plugin Execution (engine.rs)            │  │
│  │  stdin/stdout JSON protocol, timeout, isolation   │  │
│  └───────────────────────┬───────────────────────────┘  │
│                          │                               │
│  ┌───────────────────────▼───────────────────────────┐  │
│  │            Fallback Chain Execution                │  │
│  │  Plugin → Native → NoOp (per language config)     │  │
│  └──────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

### Plugin Protocol (stdin/stdout JSON)

```json
// PluginRequest → stdin
{
  "protocol_version": 1,
  "request_id": "uuid",
  "operation": "parse",
  "language": "Python",
  "files": [{"path": "main.py", "source": "..."}],
  "config": null
}

// PluginResponse ← stdout
{
  "request_id": "uuid",
  "status": "Success",
  "symbols": [
    {"name": "MyClass", "kind": "Class", "file_path": "main.py", ...}
  ],
  "error": null,
  "metrics": {"parse_time_ms": 45, "files_processed": 1, "symbols_found": 12}
}
```

### Fallback Chain

Per-language configurable fallback chain:

```
Language: Python
Default: [Plugin("parser-py"), Native, NoOp]

Execution flow:
  1. Try Plugin "parser-py" → on failure → log warning → continue
  2. Try Native (built-in tree-sitter) → on failure → log warning → continue
  3. NoOp → return empty Vec<Symbol>
```

### Plugin Registry Schema

Registry hosted at `https://raw.githubusercontent.com/swal/xavier-plugins/main/plugins.json`

```json
{
  "registry_version": 1,
  "updated_at": "2026-07-13T12:00:00Z",
  "min_engine_version": "0.12.0",
  "plugins": [
    {
      "name": "parser-python",
      "display_name": "Python Parser",
      "description": "Tree-sitter based Python parser for Xavier code-graph",
      "version": "1.0.0",
      "author": "xavier-contributors",
      "languages": ["Python"],
      "capabilities": ["parse"],
      "platform": {
        "linux": {
          "url": "https://github.com/swal/xavier-plugins/releases/download/parser-python-v1.0.0/parser-python-x86_64-linux.tar.gz",
          "checksum": "sha256:abc123...",
          "format": "tar.gz"
        }
      },
      "min_engine_version": "0.12.0",
      "license": "MIT"
    }
  ]
}
```

## 📁 File Structure

### New Files to Create (`code-graph/src/plugin/`)

| File | Purpose |
|------|---------|
| `mod.rs` | Module re-exports, PluginManager struct |
| `manager.rs` | PluginManager — lifecycle orchestrator |
| `types.rs` | PluginDescriptor, RegistryEntry, PluginHealth, traits |
| `engine.rs` | Process execution, stdin/stdout JSON protocol |
| `fallback.rs` | FallbackChain — per-language configurable chains |
| `registry.rs` | GitHubRegistry — fetch/download/checksum |
| `cache.rs` | PluginCache — local storage management |
| `discovery.rs` | LanguageDiscovery — scan plugins for languages |
| `health.rs` | PluginHealthMonitor — metrics ring buffer |
| `sdk.rs` | Plugin protocol documentation and helpers |

### New Files to Create (`code-graph/src/commands/`)

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports |
| `plugin_cmds.rs` | CLI: plugin install, update, rollback, uninstall, list, search, health |

### New Files to Create (`code-graph/src/api/`)

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports |
| `plugin_routes.rs` | axum routes for plugin management |

### Files to Modify

| File | Change |
|------|--------|
| `types.rs` | Add `Language::Other(String)` variant + `as_str()` method |
| `plugin_host.rs` | Add `#[deprecated]` wrapper delegating to PluginManager; kept for backward compat |
| `parser/mod.rs` | Rewrite `parse_source()` with fallback-chain dispatch; fix duplicate match arms |
| `indexer/mod.rs` | Minimal: swap PluginHost for PluginManager (optional, backward compat available) |
| `main.rs` | Add `Plugin(PluginCommands)` subcommand + plugin API routes |
| `lib.rs` | Add `pub mod plugin;`, `pub mod commands;`, `pub mod api;` |
| `Cargo.toml` | Add deps: `reqwest`, `sha2`, `flate2`, `tar`, `zip`, `chrono`, `semver` |

## 🔌 Integration Points

### 1. `Language` enum extension (types.rs)

```rust
pub enum Language {
    Rust, TypeScript, JavaScript, Python, Go, Java, C, Cpp,
    Other(String),  // NEW: dynamically discovered from plugins
    Unknown,
}
```

### 2. `parse_source()` rewrite (parser/mod.rs)

```rust
pub async fn parse_source(
    source: &str, lang: &Language, file_path: &str,
    plugin_manager: Option<&PluginManager>,
) -> Result<Vec<Symbol>> {
    // 1. Get fallback chain for language
    // 2. Try each step: Plugin → Native → NoOp
    // 3. Return on first success
    // 4. Log warnings on each failure
}
```

### 3. PluginManager replaces PluginHost (manager.rs)

```rust
pub struct PluginManager {
    installed: RwLock<HashMap<String, PluginDescriptor>>,
    language_map: RwLock<HashMap<String, Vec<String>>>,
    fallback: RwLock<HashMap<Language, Vec<FallbackStep>>>,
    engine: Arc<ProcessEngine>,
    registry: Arc<GitHubRegistry>,
    health: Arc<PluginHealthMonitor>,
    cache: Arc<PluginCache>,
}
```

## 📋 Implementation Phases

### Phase 1: Foundation (Days 1-2)
- Add `Language::Other(String)` + `as_str()` to types.rs
- Create `plugin/types.rs` with all traits and types
- Create `plugin/mod.rs` with PluginManager skeleton
- Fix duplicate match arms bug in parser/mod.rs

### Phase 2: Plugin Engine (Days 3-5)
- Port existing `parse_with_plugin()` → `engine.rs`
- Add timeout enforcement, process isolation, capabilities check
- Engine fallback: WASM (future) → Subprocess (now)

### Phase 3: Fallback Chain (Days 5-7)
- Implement FallbackChain with config persistence (~/.config/code-graph/fallback.json)
- Rewrite parse_source() to iterate fallback steps
- Wire into Indexer and existing plugin_host.rs wrapper

### Phase 4: Lifecycle Management (Days 8-11)
- PluginManager install/update/rollback/uninstall
- PluginCache: archive extraction, version history
- Plugin binary verification on startup

### Phase 5: Registry Client (Days 11-13)
- GitHubRegistry: fetch index, search, download, checksum verification
- Local index cache with TTL
- Offline mode: use cached plugins

### Phase 6: Discovery (Days 13-14)
- LanguageDiscovery: scan all installed plugins
- Dynamic Language::Other resolution
- Extension→language mapping from plugins

### Phase 7: CLI Commands (Days 15-17)
- `code-graph plugin list|search|install|update|rollback|uninstall|health`
- Table output for list, progress bars for install/update

### Phase 8: Health Monitoring (Days 17-19)
- PluginHealthMonitor with ring buffer
- Background health check task (60s interval)
- Metrics: avg latency, error rate, uptime

### Phase 9: API Endpoints (Days 19-21)
- GET/POST/DELETE routes for plugin management
- Health dashboard endpoint
- Fallback chain configuration endpoints

**Total estimated effort: ~3 weeks (single developer full-time)**

## 📊 Success Metrics

| Metric | Target |
|--------|--------|
| Plugin binary cold start | < 100ms |
| Plugin parse (1000LOC file) | < 5s |
| Fallback latency (plugin→native) | < 10s |
| Health check interval | 60s |
| Max installed plugins | 50 |
| Registry download (100KB plugin) | < 3s |
| Memory overhead per idle plugin | < 10MB |
| CPU overhead per parse call | < 50ms |

## 🔐 Security Considerations

- **Checksum verification**: SHA-256 of every downloaded archive
- **Process isolation**: Separate OS process per plugin execution
- **Timeout enforcement**: Configurable (default: 30s), forced kill after timeout
- **No network from plugins**: Plugins receive source via stdin only
- **Resource limits**: Via `rlimit` on Linux (address space, file descriptors, processes)
- **Version pinning**: Users pin plugin versions, no auto-update without consent

## 🔄 Graceful Degradation

```
Plugin Crash → Native fallback → NoOp (empty symbols)
     ↓              ↓
  Warning        Continue
  logged         indexing
```

- Single plugin failure NEVER crashes Xavier
- Plugin circuit breaker: 3 failures in 60s → auto-disable, alert
- Registry offline → use cached plugins + warn
- Missing plugin → transparent fallback to native

## 📦 Plugin SDK (for Plugin Authors)

Minimal protocol any language can implement:

```python
#!/usr/bin/env python3
"""Python parser plugin for Xavier code-graph"""
import sys, json, tree_sitter_python as tsp

def parse(request):
    symbols = []
    for file in request["files"]:
        parser = tsp.Parser(tsp.Language())
        tree = parser.parse(bytes(file["source"], "utf8"))
        # ... extract symbols from tree ...
    return {"symbols": symbols}

if __name__ == "__main__":
    req = json.loads(sys.stdin.readline())
    resp = {"request_id": req["request_id"], "status": "Success"}
    if req["operation"] == "parse":
        resp["symbols"] = parse(req)
    elif req["operation"] == "health":
        resp["status"] = "Success"
    print(json.dumps(resp))
```

## 🔗 Related Issues

- #97 - Code Graph Index (existing feature, will be extended)
- New: Plugin System feature (this document)
- New: Language::Other dynamic discovery
- New: Plugin registry repository setup

---

*Last updated: 2026-07-13*
*Author: Xavier Architecture Team*
