# [Ola Graph · 03] Mount memory + code graph routes in `cli/server.rs`

> Part of **Xavier Graph Explorer** wave. Thin wiring-only issue: handlers land in 01/02; this issue **only** registers routes + imports.

## Web Research Required (Jules must search the web)

Before implementing, search the internet for:
1. **Axum route method get/post path params** — search: `axum Router route get path parameter 0.7 0.8`
2. Confirm how this repo mounts other handlers from `xavier::api::*` vs `crate::cli::handlers::*` by reading nearby imports in `src/cli/server.rs`

## Exact Technical Context

- **File**: `src/cli/server.rs` (~1093 lines)
- **`protected_routes` memory block** starts ~line **409**:
```rust
.route("/memory/search", post(search_handler))
// ... more /memory/* routes through ~420
.route("/memory/timeline/query", post(timeline_query_handler))
```
- **Code routes** ~lines **430–443**:
```rust
.route("/code/stats", get(code_stats_handler))
.route("/code/hubs", get(code_hubs_handler))
.route("/code/hotspots", get(code_hotspots_handler))
```
- **WorkspaceContext layer** already applied ~line **870**: `.layer(Extension(workspace_ctx.clone()))` — required for `Extension(WorkspaceContext)` memory graph handlers
- **Panel graph already mounted** ~553: `/panel/api/graph` (roadmap) — leave untouched
- **Handlers from issue 01** (must exist on branch after 01 merges):  
  `xavier::api::graph::{memory_graph_entity, memory_graph_relations, memory_graph_list_entities, memory_graph_view}`  
  (exact names as implemented in PR from issue 01 — read `src/api/graph.rs` after rebase)
- **Handler from issue 02**: `code_graph_view_handler` in `crate::cli::handlers::code` (already `pub use code::*` via handlers/mod.rs)

> CRITICAL:
> - This issue must be based on a branch that **already includes** Ola Graph · 01 and · 02 (or rebase onto them).
> - Touch **only** `src/cli/server.rs` (and import lines at top of that file).
> - DO NOT reimplement handlers here.
> - DO NOT touch panel-ui.

## Problem

Even with handlers implemented, they are unreachable until registered on the Axum router next to existing `/memory/*` and `/code/*` routes.

## Acceptance Criteria

- [ ] Register under **protected_routes** (same auth middleware as other memory/code routes):

```rust
// Memory Knowledge Graph (EntityGraph)
.route("/memory/graph/entities", get(memory_graph_list_entities))
.route("/memory/graph/entities/{entity_id}", get(memory_graph_entity))
.route("/memory/graph/relations", get(memory_graph_relations))
.route("/memory/graph/view", get(memory_graph_view))

// Code graph canvas projection
.route("/code/graph/view", get(code_graph_view_handler))
```

- [ ] Add necessary `use` imports at top of `server.rs` (follow existing import style for `get_graph` / code handlers)
- [ ] No new business logic in this file
- [ ] Optional: tiny smoke test only if one already exists for route table — do **not** invent a large test harness
- [ ] `cargo check -p xavier` passes
- [ ] Diff ONLY `src/cli/server.rs`

## Files to Modify

| File | Change |
|---|---|
| `src/cli/server.rs` | Imports + 5 route registrations |

**DO NOT touch:** handler bodies, `panel-ui/`, `xavier-core/`, Cargo.toml

**NEVER create `.patch` / `.py` loose files.**

## Verification

```bash
cargo check -p xavier
# Manual after server up:
# curl -H "X-Xavier-Token: $T" http://127.0.0.1:8006/memory/graph/view
# curl -H "X-Xavier-Token: $T" http://127.0.0.1:8006/code/graph/view
```

## Dependencies and Merge Order

- **Depends on:** Ola Graph · 01 AND · 02 (handlers must exist)
- **Can run in parallel with:** nothing that touches `server.rs`
- **Must merge before:** Ola Graph · 05 (UI live wiring)
- **Must merge after:** 01, 02
