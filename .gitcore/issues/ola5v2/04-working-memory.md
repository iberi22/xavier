# [Ola 5v2 · 04] Multi-layer retrieve: stop all_documents() on working hot path

> **Re-launch** of weak Ola 5 · 04. Parent #496.

## Web Research Required (Jules must search the web)

1. **Working / episodic / semantic memory layers** — search: `multi-layer memory architecture working episodic semantic agents MemGPT 2024 2025`.
2. **Top-k candidate generation vs full scan** — search: `RAG candidate generation top-k limit avoid full corpus scan 2024`.
3. **SQLite recent rows by updated_at** — search: `sqlite order by updated_at desc limit rust 2024`.

Document how Xavier’s working layer should map to “recent hot set”.

## Exact Technical Context

- **Hot paths** (verify with `rg all_documents src/server`):
  - `src/server/http/api.rs` ~166: `let all_docs = workspace.workspace.memory.all_documents().await;`
  - `src/server/http/v1_api.rs` ~816, ~1377
- Multi-layer module docs: `src/server/http/api.rs` header comment multi-layer retrieval
- Prefer existing list/search APIs with **limit** before inventing new storage methods
- `WorkingMemory` type may exist — `rg WorkingMemory src/`

```rust
// ANTI-PATTERN:
let all_docs = workspace.workspace.memory.all_documents().await;

// TARGET (adapt to real API names after reading trait):
const WORKING_LAYER_LIMIT: usize = 50;
// list_recent / search with limit — NOT full dump
```

> CRITICAL: Keep `all_documents` for admin/export if needed. Only remove from multi-layer **hot** path. DO NOT touch xavier-core/. NEVER `.patch` files.

## Problem

Working layer loads the entire memory corpus → latency + token explosion and false “hot” candidates.

## Acceptance Criteria

- [ ] Working candidates capped (document constant, e.g. 50)
- [ ] No `all_documents()` on multi-layer chat/retrieve hot path
- [ ] Test or assertable unit covering the cap
- [ ] `cargo check --workspace` 0 errors
- [ ] PR lists every call site changed

## Files to Modify

| File | Change |
|---|---|
| `src/server/http/api.rs` | working layer query |
| `src/server/http/v1_api.rs` | same if needed |
| memory trait only if `list_recent` missing | minimal |

**DO NOT touch:** `tools_memory.rs` (issues 01/02), code-graph/, panel-ui/

## Verification

```bash
cargo check --workspace
rg "all_documents" src/server/http
```

## Dependencies and Merge Order

- **Depends on:** nothing hard
- **Can run in parallel with:** 05, 06, 07, 08, 09, 11
