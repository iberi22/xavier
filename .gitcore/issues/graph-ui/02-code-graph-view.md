# [Ola Graph · 02] Code graph force-view endpoint `/code/graph/view`

> Part of **Xavier Graph Explorer** wave. Provides a single nodes+links payload for panel-ui Code layer (today UI must stitch hubs + edges + find).

## Web Research Required (Jules must search the web)

Before implementing, search the internet for:
1. **Force-directed graph data model** — search: `force graph nodes links source target id format d3`
2. Review `code-graph` QueryEngine API in-repo (prefer reading source over inventing): `code-graph/src/query/mod.rs` methods `hubs`, `hotspots`, dependencies traversal
3. **Token/budget truncation patterns** already in `src/cli/handlers/code.rs` (`truncate_json_items`)

## Exact Technical Context

- **File**: `src/cli/handlers/code.rs` (~600 lines)
- **Existing handlers to mirror**:
  - `code_stats_handler` ~line **325** — `State(state): State<CliState>`
  - `code_hubs_handler` ~line **468**
  - `code_hotspots_handler` ~line **493**
  - `code_graph_edges_response` ~line **520** (private helper for deps/call-chain)
- **State access**:
```rust
let code_graph = state.code_graph.read().await;
// code_graph.db / code_graph.query / code_graph.indexer
```
- **QueryEngine** (`code-graph/src/query/mod.rs`):
  - `hubs(min_degree: u64, limit: usize)` ~line **182**
  - `hotspots(min_complexity: f32, limit: usize)` ~line **186**
  - deps via existing public methods used by `code_graph_edges_response`
- **Symbol / edge types**: `code_graph::types::{Symbol, CodeEdge, EdgeType}`
- **Edge endpoints** are **stable_id strings** (or `file:…` / `module:…` pseudo-nodes)
- **Defaults** in `src/cli/types.rs`: `default_graph_depth`, `default_graph_limit`, `default_graph_budget`, `default_min_degree`, `default_min_complexity`
- **Route registration OUT OF SCOPE** (issue 03)

> CRITICAL:
> - Use `State<CliState>` like other `/code/*` handlers (NOT WorkspaceContext).
> - Default **exclude** pseudo-nodes whose id starts with `file:` or `module:` unless `include_file_nodes=true`.
> - Do NOT read full `.xavier/codegraph.json` dump for this endpoint — use live QueryEngine/DB.
> - DO NOT touch `src/api/graph.rs`, `panel-ui/`, `xavier-core/`.

## Problem

Code graph data exists (`symbols`, `edges` in `code_graph.db`) and many `/code/*` endpoints work, but there is **no** nodes+links overview suitable for `react-force-graph-2d`. Full dump is too large for interactive UI.

## Acceptance Criteria

- [ ] Add `pub async fn code_graph_view_handler(...)` in `code.rs`
- [ ] Support **query params** (prefer `Query<>` GET):

| Param | Default | Notes |
|-------|---------|-------|
| `mode` | `overview` | `overview` \| `ego` |
| `query` | — | required for `ego` (symbol name or 64-hex stable_id) |
| `depth` | 3 | clamp 1..=8 (ego) |
| `limit` | 150 | clamp 1..=1000 nodes target |
| `edge_type` | optional | same parsing as deps handlers |
| `include_file_nodes` | false | |
| `min_degree` | 3 | overview hubs seed |

- [ ] **overview mode**:
  1. `hubs(min_degree, limit)` as seed symbols
  2. Collect incident edges among seed set (use query deps or iterate edges from query engine / db API available — if no “edges between set” API, expand each hub with depth=1 deps and keep edges where both ends in node set OR degree-1 neighbors up to limit)
  3. Build nodes from unique symbol ids (lookup symbol metadata when possible)

- [ ] **ego mode**:
  1. Use same security scan pattern as `code_find_handler` / `code_graph_edges_response` on `query`
  2. Call dependencies (or call-chain if edge_type is Calls) with depth/limit
  3. Project edges → nodes+links

- [ ] Response shape (**same canvas contract as memory view**):
```json
{
  "status": "ok",
  "layer": "code",
  "truncated": false,
  "nodes": [
    {
      "id": "<stable_id>",
      "label": "function_name",
      "kind": "Function",
      "meta": { "path": "src/...", "line": 10, "lang": "Rust", "complexity": 1.0 }
    }
  ],
  "links": [
    { "source": "...", "target": "...", "relation": "Calls", "weight": 1.0 }
  ],
  "stats": {
    "total_symbols": 0,
    "shown_nodes": 0,
    "shown_links": 0
  }
}
```
- [ ] Optionally enrich `code_stats_handler` with `total_edges` if a cheap count exists on `db.stats()` — only if already available without schema change; otherwise skip
- [ ] Unit test: pure helper that maps a small `Vec<CodeEdge>` + symbol map → nodes/links (no need for full DB if hard)
- [ ] `cargo check -p xavier` passes
- [ ] Diff ONLY files listed

## Files to Modify

| File | Change |
|---|---|
| `src/cli/handlers/code.rs` | Add `code_graph_view_handler` + helpers + tests |
| `src/cli/types.rs` (optional) | Query payload/defaults if cleaner |

**DO NOT touch:** `src/cli/server.rs` (issue 03), `src/api/graph.rs`, `panel-ui/`, `xavier-core/`, Cargo.toml dependencies

**NEVER create `.patch` / `.py` / loose root files.**

## Verification

```bash
cargo check -p xavier
cargo test -p xavier --lib code_graph_view 2>/dev/null || cargo test -p xavier --lib handlers::code
```

## Dependencies and Merge Order

- **Depends on:** nothing
- **Can run in parallel with:** Ola Graph · 01, 04
- **Must merge before:** Ola Graph · 03 (mount), · 05 (UI Code tab)
