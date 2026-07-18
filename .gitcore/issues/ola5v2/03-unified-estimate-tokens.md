# [Ola 5v2 · 03] Unified estimate_tokens helper (no new deps)

> **Re-launch** of weak Ola 5 · 03. Parent #496. Gold-standard Jules issue.

## Web Research Required (Jules must search the web)

1. **Heuristic token estimation without tokenizer** — search: `openai rule of thumb tokens characters divided by 4 2024 accuracy`, document error bands.
2. **UTF-8 chars vs bytes for budgets** — search: `rust str chars count vs len utf8 token estimate 2024`, pick `chars().count()` vs `len()`.
3. **Avoid heavy deps** — search: `tiktoken-rs crate crates.io 2025`, compare cost; **prefer zero new dependencies** unless already in workspace `Cargo.toml`.

Confirm with `rg "tiktoken|tokenizers" Cargo.toml` before adding crates.

PR must list research conclusion (recommended: `div_ceil(4)` on unicode scalar count).

## Exact Technical Context

- Scattered approx estimators:
  - `src/app/proxy_use_case.rs` ~492: `user_msg.len() / 4`
  - Other sites: `rg "len\\(\\) / 4"` under `src/`
- Prefer **NEW** `src/context/token_estimate.rs` + export in `src/context/mod.rs`
- Wire into `memory_context` response if `MCPContextResult` in `types.rs` ~140 can gain `estimated_tokens: usize` (serde default 0)

```rust
// TARGET helper
pub fn estimate_tokens(text: &str) -> usize {
    let n = text.chars().count();
    if n == 0 { 0 } else { n.div_ceil(4) }
}
```

> CRITICAL: Max **5** call-site replacements. No monorepo-wide drive-by. No new dep unless unavoidable. No xavier-core/. No `.patch` files.

## Problem

Multiple token estimators make progressive-disclosure budgets and savings claims undebuggable.

## Acceptance Criteria

- [ ] `estimate_tokens` + unit tests (empty, ascii, unicode)
- [ ] Used from MCP `memory_context` path (field or internal budget log)
- [ ] ≤5 call sites switched to helper
- [ ] `cargo check --workspace` 0 errors
- [ ] `cargo test -p xavier --lib token_estimate` passes
- [ ] PR lists every call site

## Files to Modify

| File | Change |
|---|---|
| `src/context/token_estimate.rs` (NEW) | helper + tests |
| `src/context/mod.rs` | mod export |
| `src/server/mcp/tools_memory.rs` and/or `types.rs` | wire |
| optional ≤3 other `len()/4` sites | replace |

**DO NOT touch:** BM25, code-graph/, panel-ui/, xavier-core/

## Verification

```bash
cargo check --workspace
cargo test -p xavier --lib token_estimate
```

## Dependencies and Merge Order

- **Depends on:** nothing
- **Can run in parallel with:** 01, 05, 06, 07, 09, 11
