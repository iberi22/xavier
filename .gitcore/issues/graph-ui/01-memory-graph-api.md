# [Ola Graph · 01] Memory Knowledge Graph API: list entities + canvas view

> Part of **Xavier Graph Explorer** wave. Completes the missing HTTP surface for `EntityGraph` so panel-ui can show real memory-derived entities (not the panel roadmap blob).
>
> **PR1 (Roadmap CRUD)** is separate human PR #663 — do NOT redo panel_graphs work here.

## Web Research Required (Jules must search the web)

Before implementing, search the internet for:
1. **Axum Query deserialization for optional Vec** — search: `axum Query Option Vec String deserialize 2024` (comma-separated or repeated keys). Prefer a simple `Option<String>` + split on `,` for `relation_types` / `entity_type` filters if `Vec` binding is fragile.
2. **HTTP status codes for not-found vs empty graph** — search: `REST API empty collection vs 404 best practice` — empty graph = **200** with empty arrays; missing entity id = **404**.
3. Review existing handler style in this repo: `src/server/panel/storage.rs` `get_graph` / `save_graph`.

## Exact Technical Context

- **File**: `src/api/graph.rs` (**133 lines** today)
- **Module already exported**: `src/api/mod.rs` → `pub mod graph;`
- **Handlers that exist but are NOT mounted anywhere**:
  - `memory_graph_entity` ~line **63**
  - `memory_graph_relations` ~line **98**
- **EntityGraph public methods** (`src/memory/entity_graph/mod.rs`):
  - `all_entities()` ~line **71**
  - `all_relations()` ~line **75**
  - `entity_neighbors(...)` ~line **110**
  - `relations_for_entity(...)` ~line **85**
- **Access pattern** (already used in this file):
```rust
workspace.workspace.entity_graph.all_entities().await
// Extension(WorkspaceContext) — WorkspaceContext is layered in server.rs ~870
```
- **Types**: `EntityRecord`, `EntityRelationRecord`, `EntityType`, `GraphDirection` in `src/memory/entity_graph/types.rs`
- **Router mount is OUT OF SCOPE** (issue 03 owns `src/cli/server.rs`)

> CRITICAL:
> - Keep `Extension(WorkspaceContext)` extractors (do not switch to `CliState` — entity_graph lives on WorkspaceState).
> - Error JSON with **HTTP 200** is wrong for missing entity — use `StatusCode::NOT_FOUND`.
> - DO NOT touch `panel_graphs`, `panel-ui/`, `xavier-core/`, `code-graph/`.

## Problem

`EntityGraph` can list entities/relations and traverse neighbors, but there is **no mounted HTTP API** and **no list/view projection** for force-graph UIs. Docs claim `/memory/graph` routes that do not exist. Panel "Knowledge Graph" currently only talks to `/panel/api/graph` (roadmap JSON blob).

## Acceptance Criteria

- [ ] Add `memory_graph_list_entities` handler:
  - `GET` query params: `q` (optional substring on name/normalized_name), `entity_type` (optional string matching `EntityType::as_str()`), `limit` (default 500, max 2000), `offset` (default 0)
  - Response **200**:
```json
{
  "status": "ok",
  "total": 123,
  "count": 50,
  "entities": [ /* EntityRecord */ ]
}
```
  - Sort entities by `trust_score` desc then `name` for stable UI

- [ ] Add `memory_graph_view` handler (force-graph projection):
  - Query: `limit_nodes` (default 500, max 2000), `min_weight` (default 0.0), `entity_id` (optional), `max_depth` (default 2, only if entity_id set), `entity_type` (optional filter on nodes)
  - If `entity_id` set: build ego-graph via `entity_neighbors` (include center + nodes from traversal/incoming/outgoing)
  - Else: take up to `limit_nodes` entities from `all_entities`, include relations from `all_relations` where **both** ends are in the node set and `weight >= min_weight`
  - Response **200**:
```json
{
  "status": "ok",
  "layer": "memory",
  "truncated": false,
  "nodes": [
    { "id": "...", "label": "Name", "kind": "person", "description": null, "trust_score": 0.5, "memory_count": 1 }
  ],
  "links": [
    { "source": "...", "target": "...", "relation": "co_occurs_with", "weight": 0.3, "confidence_score": 0.5 }
  ],
  "stats": { "entities": 10, "relations": 4, "shown_nodes": 10, "shown_links": 4 }
}
```
  - Set `truncated: true` when filtered by limit

- [ ] Fix `memory_graph_entity` missing-entity path to return **404** with `{ "status":"error", "message":... }` (use `.into_response()` with StatusCode)

- [ ] Unit tests in `src/api/graph.rs` `#[cfg(test)]` OR `src/memory/entity_graph` already has graph tests — prefer pure projection helper tests:
  - empty graph → empty nodes/links
  - after `upsert_memory` with text containing two entities → view has ≥1 node (if extractor finds them) OR test projection helper with synthetic EntityRecord vectors
  - limit truncates and sets truncated flag

- [ ] `cargo check --workspace` 0 errors (or at least `cargo check -p xavier` if workspace has known platform noise)
- [ ] Diff touches ONLY files listed below

## Suggested implementation sketch

```rust
// Project domain records → canvas DTOs (keep in graph.rs)
#[derive(Serialize)]
struct GraphViewNode {
    id: String,
    label: String,
    kind: String, // entity.entity_type.as_str()
    description: Option<String>,
    trust_score: f32,
    memory_count: usize,
}

#[derive(Serialize)]
struct GraphViewLink {
    source: String,
    target: String,
    relation: String,
    weight: f32,
    confidence_score: f32,
}

pub async fn memory_graph_view(
    Extension(workspace): Extension<WorkspaceContext>,
    Query(q): Query<GraphViewQuery>,
) -> impl IntoResponse { /* ... */ }

pub async fn memory_graph_list_entities(
    Extension(workspace): Extension<WorkspaceContext>,
    Query(q): Query<GraphListQuery>,
) -> impl IntoResponse { /* ... */ }
```

Export new handlers as `pub` so issue 03 can `use xavier::api::graph::{...}`.

## Files to Modify

| File | Change |
|---|---|
| `src/api/graph.rs` | Add list + view handlers; fix 404 status; optional small pure helpers + unit tests |

**DO NOT touch:** `src/cli/server.rs` (issue 03), `panel-ui/`, `src/server/panel/`, `xavier-core/`, `Cargo.toml` (no new deps), code-graph crate

**NEVER create `.patch` / `.py` / `part1.rs` loose files in repo root.**
Edit `.rs` files directly. If `git diff --stat` shows 0 files, PR will be rejected.

## Verification

```bash
cargo check -p xavier
cargo test -p xavier --lib api::graph
# or: cargo test -p xavier --lib entity_graph
```

## Dependencies and Merge Order

- **Depends on:** nothing (EntityGraph already on WorkspaceState)
- **Can run in parallel with:** Ola Graph · 02, 04
- **Must merge before:** Ola Graph · 03 (route mount), · 05 (UI), · 06 (durability can start after this)
