> Parent EPIC: {{EPIC}}

# [Ola 11 · 01] Headless code_* tools: wire real handlers (replace 501)

> Wave **Ola 11 — Harden Residuals**. Follow-up to Ola 10 #1108/#1120 which shipped honest 501.

## Current State (MEDIBLE)
- `src/server/headless/routes.rs` (~210 lines)
- `execute_tool`: `code_*` → HTTP 501 JSON `{code:501}`
- Unit tests assert 501 for `code_scan`
- Live HTTP/CLI code path already works via `src/cli/handlers/code.rs` + `code_graph`

## Desired State (DELTA)
- `code_scan` / `code_find` / `code_context` / `code_stats` execute real logic (or thin wrappers calling existing code-graph/query APIs available in process)
- Replace 501 tests with assertions on real success or structured error from real path (not mock `"executed"`)
- If full CliState is unavailable, construct minimal in-process indexer/query the same way other server paths do — do NOT return fake success

## Web Research Required
1. Read current `execute_tool` + headless App/state in same module tree
2. search: `axum tool dispatch share handler state rust 2025`
3. Read how `src/cli/handlers/code.rs` obtains `QueryEngine` (read-only)

## Exact Technical Context
- CRITICAL: ONLY modify `src/server/headless/routes.rs`
- Do not invent MCP tools (Ola 10 already did)
- Prefer reusing existing public functions over copy-paste

## Problem
Agents in headless mode still cannot scan/find code after Ola 10 deferred wiring.

## Acceptance Criteria
- [ ] `code_scan` no longer returns 501 when wiring succeeds
- [ ] No `"status":"executed"` mock for `code_*`
- [ ] Unit tests updated; `cargo test -p xavier --lib server::headless::routes` passes
- [ ] `cargo check -p xavier` 0 errors
- [ ] Only listed file changed

## Files to Modify
| File | Change | Risk |
|------|--------|------|
| `src/server/headless/routes.rs` | Wire real code_* | HIGH |

## DO NOT touch
- `src/cli/handlers/code.rs`, `src/server/mcp/**`, `src/adapters/**`, `code-graph/**` (read-only ok)
- `.gitcore/features.json`

## Verification
```bash
CARGO_TARGET_DIR=/tmp/rt cargo check -p xavier
CARGO_TARGET_DIR=/tmp/rt cargo test -p xavier --lib server::headless::routes -- --nocapture
```

## Dependencies & Merge Order
- **Parallel with:** 02–11
- **Expected effort:** Large
