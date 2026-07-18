# [Ola 5v2 · 10] parser-python plugin template (stdin/stdout JSON)

> **Re-launch** of #489 plugin half. Depends on layout from 09.

## Web Research Required (Jules must search the web)

1. **tree-sitter Python AST node types** — search: `tree-sitter python function_definition class_definition node types 2024 2025`
2. **Language tool plugins over stdin/stdout JSON** — search: `compiler plugin stdin stdout json protocol design 2024`
3. **tree-sitter Python bindings** — search: `tree-sitter-python pypi 2025` — optional; stub symbols allowed if dependency is heavy

## Exact Technical Context

- Protocol: `code-graph/src/plugin/types.rs` — `PluginRequest`, `PluginResponse`, symbol fields
- IO contract: read `code-graph/src/plugin/engine.rs` + `discovery.rs` **before** coding (exact JSON field names)
- Place under: `code-graph/fixtures/xavier-plugins/templates/parser-python/`

```python
# Skeleton after reading Rust types — field names MUST match serde:
# stdin JSON → health → success status
# parse → symbols: [{name, kind, lang, file_path, start_line, end_line, ...}]
```

> CRITICAL: Match Rust serde names exactly. Health path required. DO NOT break `cargo check --workspace`. NEVER `.patch` in repo root.

## Problem

No first-party plugin author template; registry cannot document a real plugin shape.

## Acceptance Criteria

- [ ] `plugin.py` handles health + minimal parse (stub OK)
- [ ] `example_request.json` + `example_response.json`
- [ ] README run instructions
- [ ] `cargo check --workspace` still green

## Files to Modify

| File | Change |
|---|---|
| `code-graph/fixtures/xavier-plugins/templates/parser-python/**` (NEW) | template |

## Verification

```bash
python code-graph/fixtures/xavier-plugins/templates/parser-python/plugin.py < example_request.json
cargo check --workspace
```

## Dependencies and Merge Order

- **Depends on:** 09 (directory conventions)
- **Can run in parallel with:** 01–08 after 09 merges
