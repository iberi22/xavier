# [Ola 5 · 01] MCP mem_search: structured fat index (no Metadata dump)

> Part of **Ola 5 Cost** / EPIC #496 / advances #497. Wave: progressive disclosure finish.

## Web Research Required
1. MCP structuredContent tool results 2025 — search: `MCP tool structuredContent result schema 2025`
2. MemGPT progressive disclosure agent patterns

## Exact Technical Context
- File: `src/server/mcp/tools_memory.rs` mem_search arm ~237-290
- Today returns `MCPContent::Text` with `Metadata: {:?}` which bloats tokens
- `include_content` already defaults **false** (line ~249) — KEEP
- Pattern for structured: `MCPToolResult::structured` / `MCPContent::Structured` used in memory_context (~758)
- Types: `MCPSearchResult` already exists in module (~782)

## Problem
Agents pay tokens for Debug-printed metadata even when fat-search is intended.

## Acceptance Criteria
- [ ] mem_search returns **structuredContent** array of candidates: `{id, path, score, snippet, kind}` only
- [ ] snippet ≤ 100 chars; **no** full content unless `include_content=true`
- [ ] **no** `Metadata: {:?}` dump when include_content=false
- [ ] Unit test: include_content false → no long body fields
- [ ] `cargo check --workspace` 0 errors
- [ ] Diff only tools_memory.rs + tests; empty PR forbidden
- [ ] NEVER .patch files; DO NOT touch xavier-core/

## Files
| File | Change |
|---|---|
| `src/server/mcp/tools_memory.rs` | structured mem_search |
| `src/server/mcp/tests.rs` | test fat search shape |

## Merge order
Parallel with 02, 03 if not same lines — prefer merge **01 before 02**.
