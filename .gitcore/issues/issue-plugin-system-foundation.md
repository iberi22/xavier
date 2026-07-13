# Issue: Plugin System — Foundation Phase

## Labels
`enhancement`, `plugin-system`, `P1-foundation`

## Description
Extract language-specific parser logic from Xavier's code-graph into a proper plugin system. This is Phase 1 (Foundation) of the Plugin System feature.

## Changes Required

### 1. `code-graph/src/types.rs` — Language enum extension
Add `Language::Other(String)` variant for dynamically discovered languages:

```rust
pub enum Language {
    Rust, TypeScript, JavaScript, Python, Go, Java, C, Cpp,
    Other(String),  // NEW: dynamically discovered from plugins
    Unknown,
}
```

Also add `fn as_str(&self) -> &str` method.

### 2. `code-graph/src/plugin/types.rs` — New file
Create plugin types module with:

- `PluginDescriptor` — metadata about installed plugin
- `RegistryEntry` — plugin index entry from GitHub
- `PluginHealth` — health status struct
- `PluginMetrics` — aggregated metrics
- `FallbackStep` — enum: Plugin(String), Native, NoOp
- `HealthStatus` — Healthy, Degraded, Unhealthy, Unknown
- Traits: `PluginLifecycle`, `PluginEngine`, `PluginRegistryClient`, `FallbackResolver`, `HealthMonitor`

### 3. `code-graph/src/plugin/mod.rs` — New file
Module root with `PluginManager` struct skeleton.

### 4. Fix duplicate match arms
In `code-graph/src/parser/mod.rs`, lines 82-89 have duplicate Python/TypeScript match arms:

```rust
Language::Python => { ... }  // line 82 (duplicate)
Language::TypeScript | Language::JavaScript => { ... }  // line 86 (duplicate)
```

Remove duplicates.

## Definition of Done
- [ ] `Language::Other(String)` compiles and implements all existing traits
- [ ] `plugin/types.rs` defines all types and traits
- [ ] `plugin/mod.rs` exports `PluginManager` struct
- [ ] `cargo build --release` passes
- [ ] Existing tests still pass
- [ ] Duplicate match arms removed
- [ ] Feature spec read at `.gitcore/features/FEATURE-plugin-system.md`

## Related
- Feature: `feat-plugin-system`
- Spec: `.gitcore/features/FEATURE-plugin-system.md`
- Depends on: Nothing (Phase 1, no external deps)
