# [Ola 5v2 · 02] memory_context: max_chars_per_doc param + slim sources metadata

> **Re-launch** of residual #497 / weak Ola 5 · 02. Skill: jules-async-orchestration gold standard.
> Parent EPIC: #496.

## Web Research Required (Jules must search the web)

1. **Per-document context budgets in RAG** — search: `RAG context budget max tokens per document fair share vs fixed cap 2024 2025`. Choose and document strategy.
2. **MCP structured content + provenance** — search: `MCP structuredContent sources provenance schema 2025`, read https://modelcontextprotocol.io/specification (or current docs).
3. **Serde omit heavy fields** — search: `serde skip_serializing_if Option Value rust 2024` for trimming `metadata` maps.

PR description must include 2–3 research bullets.

## Exact Technical Context

- **File**: `src/server/mcp/tools_memory.rs` (~938 lines)
- **Arm**: `memory_context` ~714–862
- **Already done** (do not regress):
  - Global `max_chars` default 4000 / absolute 16000 (~727–731)
  - Fair-share `per_doc_limit = max_chars / expanded.len()` (~808–818)
  - Final hard truncate (~831–840)
  - `ids` array page-in (~733–740)
- **Gaps**:
  1. No explicit `max_chars_per_doc` in tool schema (~213–224)
  2. Sources still clone full metadata:

```rust
// ~781-804 CURRENT — progressive disclosure leak:
sources.push(MCPSearchResult {
    id: doc.id.clone().unwrap_or_default(),
    path: doc.path.clone(),
    score: 0.0,
    snippet: doc.content.chars().take(200).collect(),
    provenance: MCPProvenance { /* ... */ },
    metadata: doc.metadata.clone(), // REMOVE full clone
});
```

Target metadata policy: empty `{}` OR only `{"kind": "..."}`.

```rust
// TARGET per_doc_limit:
let per_doc_limit = match max_chars_per_doc {
    Some(n) => n.min(max_chars / expanded.len().max(1)),
    None => max_chars / expanded.len().max(1),
};
```

> CRITICAL: Keep `ids[]` path. DO NOT change BM25/RRF. DO NOT touch `xavier-core/`. NEVER `.patch` files.

## Problem

Page-in responses still embed full metadata maps in `sources[]`, re-inflating tokens after a careful content truncate.

## Acceptance Criteria

- [ ] Optional arg `max_chars_per_doc` in schema + handler
- [ ] When set, caps each doc body; when unset, keep fair-share
- [ ] `sources[].metadata` is empty or kind-only (no full clone)
- [ ] Integration test asserts slim sources + per-doc cap
- [ ] `cargo check --workspace` 0 errors
- [ ] Empty PR forbidden

## Files to Modify

| File | Change |
|---|---|
| `src/server/mcp/tools_memory.rs` | schema + limits + metadata trim |
| `src/server/mcp/tests.rs` | tests |

**DO NOT touch:** code-graph/, panel-ui/, proxy_use_case.rs, xavier-core/

## Verification

```bash
cargo check --workspace
cargo test -p xavier --lib -- memory_context
```

## Dependencies and Merge Order

- **Depends on:** soft after 01 if both edit tests.rs
- **Can run in parallel with:** 03, 05, 06, 07, 09
