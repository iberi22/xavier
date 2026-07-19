# [Ola Graph · 06] Persist EntityGraph to SQLite and reload on boot

> Part of **Xavier Graph Explorer** wave. Makes Memory KG survive restarts without full document reindex.

## Web Research Required (Jules must search the web)

Before implementing, search the internet for:
1. **SQLite upsert entity graph patterns** — search: `sqlite INSERT OR REPLACE entity relationship graph`
2. **Dual-write vs rebuild tradeoffs** — search: `event sourcing vs dual write knowledge graph persistence`
3. Review current dual path in-repo: light extractor `src/memory/sqlite_vec_store/graph.rs` vs NER `src/memory/entity_graph/`

## Exact Technical Context

- **In-memory EntityGraph**: `src/memory/entity_graph/mod.rs`
  - `export_json` / `import_json` ~lines **139–149**
  - Boot today: `EntityGraph::new()` empty then async reindex (`src/workspace/state.rs` ~231–253 area)
  - Index path: `index_memory_entities` ~line **600** → `entity_graph.upsert_memory`
- **SQLite tables already exist** (v4 migration `src/storage/migrations.rs` ~156–188):
  - `entities(id, name, entity_type, properties, language_family, workspace_id)`
  - `relations(id, source_id, target_id, relation_type, properties, weight, confidence_score, …, workspace_id)`
  - **Note:** schema is lighter than `EntityRecord` (missing trust_score, aliases as first-class columns) — store extended fields in `properties` JSON if needed
- **Light retrieval writer** (different extractor!): `src/memory/sqlite_vec_store/graph.rs` `sync_memory_entities` — **do not break** `graph_hops` / hybrid search
- **Connection access patterns**: `ConnectionManager::global().with_conn(...)` used by panel storage

> CRITICAL:
> - Do **not** delete or repurpose light `sync_memory_entities` without preserving graph_hops behavior.
> - Prefer **dedicated** tables `kg_entities` / `kg_relations` **OR** a single snapshot blob table `entity_graph_snapshots(workspace_id, data_json, updated_at)` if dual-writing into `entities` risks breaking retrieval.
> - **Recommended simpler approach for this issue:** snapshot via existing `export_json`/`import_json` into new table `entity_graph_snapshots` — faster, lower risk than unifying extractors.
> - DO NOT touch panel-ui.
> - DO NOT touch `xavier-core/`.

## Problem

EntityGraph lives only in RAM (rebuilt from all memories on boot). UI Memory KG can appear empty after restart until reindex finishes, and expensive rebuilds do not scale.

## Acceptance Criteria

### Preferred design (snapshot — default if dual-write too risky)

- [ ] Migration: `CREATE TABLE IF NOT EXISTS entity_graph_snapshots (
    workspace_id TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    updated_at TEXT NOT NULL
  );` via existing migration system (`src/storage/migrations.rs` next version)
- [ ] After successful `upsert_memory` / `index_memory_entities`, debounce or immediate `export_json` + save snapshot for workspace
- [ ] On `WorkspaceState::new` (or equivalent boot): if snapshot exists, `import_json` **before** or instead of full reindex; if import ok, skip full reindex OR still reindex in background without blocking
- [ ] Unit test: upsert → export → new EntityGraph import → same entity count
- [ ] Integration-ish test with temp sqlite if feasible

### Alternative (full dual-write)

Only if snapshot rejected by review: map EntityRecord ↔ entities/relations carefully without breaking graph_hops.

### Common

- [ ] `cargo check -p xavier` passes
- [ ] `cargo test -p xavier --lib entity_graph` passes
- [ ] Document in module rustdoc: how persistence works

## Files to Modify

| File | Change |
|---|---|
| `src/storage/migrations.rs` | New table (snapshot or columns) |
| `src/memory/entity_graph/mod.rs` and/or `storage.rs` | load/save helpers |
| `src/workspace/state.rs` | boot load + save after index |
| Possibly `src/memory/sqlite_vec_store/*` | only if dual-write chosen |

**DO NOT touch:** `panel-ui/`, `src/cli/server.rs`, `src/api/graph.rs` handlers (unless tiny call), `xavier-core/`

**NEVER create root loose files.**

## Verification

```bash
cargo check -p xavier
cargo test -p xavier --lib entity_graph
cargo test -p xavier --lib workspace
```

## Dependencies and Merge Order

- **Depends on:** Ola Graph · 01 preferred (API already useful); can land after 01 even if UI pending
- **Can run in parallel with:** Ola Graph · 04, 05 (different stack)
- **Must merge before:** Ola Graph · 07
