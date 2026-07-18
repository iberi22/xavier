# [Ola 5v2 · 09] Plugin registry fixtures: plugins.json + schema (in-repo)

> **Re-launch** of #489 (registry half). Gold standard. Live `swal/xavier-plugins` may be out of Jules sandbox scope.

## Web Research Required (Jules must search the web)

1. **JSON Schema Draft 2020-12** — search: `json schema draft 2020-12 object additionalProperties 2024`, optional validate fixture.
2. **GitHub raw registry indexes** — search: `github raw.githubusercontent.com package registry index json pattern 2024`.
3. **SHA-256 checksum encoding** — search: `sha256 hex digest prefix sha256: verify downloads 2024`.

## Exact Technical Context

- Types: `code-graph/src/plugin/types.rs` — `RegistryEntry` / registry section ~174+
- Manager stub: `code-graph/src/plugin/manager.rs` `DefaultRegistry` ~258–292 returns not found
- Trait `PluginRegistry` methods ~269+
- Platform keys must match whatever `current_platform()` uses if present (`rg current_platform code-graph`)

Create tree:
```
code-graph/fixtures/xavier-plugins/
  plugins.json
  plugins.schema.json
  README.md
```

`plugins.json` must serde-deserialize into the real Rust types used by the registry.

> CRITICAL: Do **not** claim you created the GitHub org repo unless you actually did. Prefer fixtures + tests. DO NOT touch xavier-core/. NEVER `.patch` files.

## Problem

Registry client code exists but has no canonical fixture; live remote URL is unusable → plugins path untested.

## Acceptance Criteria

- [ ] Fixture JSON matches `RegistryEntry` fields exactly
- [ ] Schema file documents required fields
- [ ] Unit test: `serde_json::from_str` → `Vec<RegistryEntry>` or index type
- [ ] README notes production URL override
- [ ] `cargo test -p code-graph` passes
- [ ] Empty PR forbidden

## Files to Modify

| File | Change |
|---|---|
| `code-graph/fixtures/xavier-plugins/*` (NEW) | fixture + schema + README |
| plugin tests | roundtrip |

**DO NOT touch:** main MCP tools, panel-ui/

## Verification

```bash
cargo test -p code-graph
python -c "import json; json.load(open('code-graph/fixtures/xavier-plugins/plugins.json'))"
```

## Dependencies and Merge Order

- **Depends on:** nothing
- **Must merge before:** 10
