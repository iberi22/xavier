# [Ola 5v2 · 01] Tests: progressive disclosure for structured mem_search + memory_context(ids)

> **Re-launch** of weak Ola 5 issues and residual #497. Wave: **Ola 5v2** under skill `jules-async-orchestration`.
> Parent EPIC: #496 (token savings).

## Web Research Required (Jules must search the web)

Before writing any code, Jules MUST search the internet and apply findings:

1. **MCP structuredContent tool results** — search: `MCP model context protocol tool result structuredContent schema 2024 2025`, read https://modelcontextprotocol.io/docs and note how clients parse structured vs text content blocks.
2. **Axum/tower ServiceExt oneshot JSON-RPC tests** — search: `axum tower ServiceExt oneshot post_json test 2024`, compare with this repo’s MCP test helpers.
3. **MemGPT progressive disclosure / page-in** — search: `MemGPT progressive disclosure memory search page-in ids 2024 2025`, ensure tests encode index-first then page-in-by-id.

In the PR description, list 2–3 bullets: what you learned and how it shaped the tests.

## Exact Technical Context

- **Baseline on main**: `mem_search` already returns **structured** fat candidates (orchestrator). Do **not** re-implement search unless tests fail.
- **File**: `src/server/mcp/tools_memory.rs` (~938 lines)
  - `mem_search` arm ~237–297 → `MCPToolResult::structured` with `candidates[]`
  - `memory_context` arm ~714–862 → supports `ids` (~733–740), `max_chars`, fair-share `per_doc_limit` (~808)
- **File**: `src/server/mcp/types.rs` — `MCPToolResult::structured` ~line 174
- **Tests pattern**: `src/server/mcp/tests.rs` helpers `test_state()`, `test_router()`, `post_json()`, `get_json_body()` (see `tools_health_check_returns_structured` ~604+)

Current `mem_search` payload shape:
```json
{
  "query": "...",
  "include_content": false,
  "count": 1,
  "candidates": [
    { "id": "...", "path": "...", "score": 0.0, "snippet": "...", "kind": "..." }
  ]
}
```

> CRITICAL: DO NOT touch `xavier-core/`. DO NOT rewrite BM25/RRF. NEVER create `.patch` / `.py` / `part1.rs` in repo root. Empty PRs rejected.

## Problem

Progressive disclosure exists in production code but is not locked by CI tests, so regressions (full body dumps) can return unnoticed.

## Acceptance Criteria

- [ ] Test `mem_search_fat_index_has_no_full_content_by_default`:
  1. Seed ≥1 memory with long body
  2. Call MCP `tools/call` name `mem_search` without `include_content`
  3. Assert structured candidates present
  4. Assert no `content` field on candidates when include_content is false
  5. Assert full memory body string is **not** embedded in the JSON response
- [ ] Test `memory_context_by_ids_only_returns_requested_ids` using `ids: [id]`
- [ ] `cargo check --workspace` passes with 0 errors
- [ ] Relevant `cargo test -p xavier --lib` filter passes
- [ ] PR description includes Web Research findings

## Files to Modify

| File | Change |
|---|---|
| `src/server/mcp/tests.rs` | New tests (preferred only file) |

**DO NOT touch:** `tools_memory.rs` unless a 1-line testability fix is required.

**NEVER create `.patch` / `.py` loose files in repo root.** Edit `.rs` directly.

## Verification

```bash
cargo check --workspace
cargo test -p xavier --lib -- mcp
```

## Dependencies and Merge Order

- **Depends on:** nothing (structured search already on main)
- **Can run in parallel with:** 03, 05, 06, 07, 08, 09, 11
- **Merge before:** 14
