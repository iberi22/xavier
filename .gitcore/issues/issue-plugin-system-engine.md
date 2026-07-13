# Issue: Plugin System — Engine + Fallback Chain

## Labels
`enhancement`, `plugin-system`, `P2-engine`

## Description
Implement the plugin execution engine and fallback chain system for Xavier's code-graph plugin system.

## Changes Required

### 1. `code-graph/src/plugin/engine.rs` — New file
Port existing `parse_with_plugin()` from `plugin_host.rs`:

```rust
pub struct ProcessEngine {
    default_timeout: Duration,  // default: 30s
    max_output_size: usize,     // default: 10MB
}

impl ProcessEngine {
    pub async fn execute(
        &self,
        plugin: &PluginDescriptor,
        request: PluginRequest,
    ) -> Result<PluginResponse>;
    
    pub async fn health_check(
        &self,
        plugin: &PluginDescriptor,
    ) -> Result<PluginHealth>;
}
```

Key features:
- stdin/stdout JSON protocol (matching current PluginRequest/PluginResponse)
- Timeout enforcement with forced kill
- Process isolation (separate OS process)
- Capabilities check before execution
- Resource limits (rlimit on Linux)

### 2. `code-graph/src/plugin/fallback.rs` — New file

```rust
pub struct FallbackChain {
    chains: RwLock<HashMap<Language, Vec<FallbackStep>>>,
}

impl FallbackChain {
    pub fn chain_for(&self, lang: &Language) -> Vec<FallbackStep>;
    pub fn set_chain(&mut self, lang: Language, chain: Vec<FallbackStep>);
    pub fn reset_defaults(&mut self);
    pub fn save(&self) -> Result<()>;
    pub fn load(path: &Path) -> Result<Self>;
}
```

Default chains:

| Language | Default Chain |
|----------|--------------|
| Rust | `[Native]` (or `[Plugin("parser-rust"), Native]` when plugin installed) |
| TypeScript | `[Plugin("parser-ts"), Native, NoOp]` |
| Python | `[Plugin("parser-py"), Native, NoOp]` |
| Go/Java/C/Cpp | `[Plugin("parser-<lang>"), Native, NoOp]` |
| Other(X) | `[Plugin("parser-" + X.lower()), NoOp]` |

### 3. `code-graph/src/parser/mod.rs` — Rewrite parse_source()
Replace hardcoded dispatch with fallback-chain-aware version:

```rust
pub async fn parse_source(
    source: &str, lang: &Language, file_path: &str,
    plugin_manager: Option<&PluginManager>,
) -> Result<Vec<Symbol>> {
    // 1. Get fallback chain for language
    // 2. Try each step: Plugin → Native → NoOp
    // 3. Log warning on each failure
    // 4. Return on first success or empty
}
```

### 4. Mark `plugin_host.rs` as deprecated
Add `#[deprecated(since = "0.7.0", note = "Use PluginManager")]` to `PluginHost`.

## Definition of Done
- [ ] `engine.rs` executes plugins with proper isolation and timeout
- [ ] `fallback.rs` provides per-language configurable chains
- [ ] `parse_source()` uses fallback chain instead of hardcoded dispatch
- [ ] Plugin crash → logs warning → continues to next fallback
- [ ] Config persists to `~/.config/code-graph/fallback.json`
- [ ] Backward compatible: old PluginHost still works
- [ ] `cargo build --release` passes
- [ ] All existing tests pass
