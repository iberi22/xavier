# Xavier Cognitive Memory System

The `src/memory` module provides durable memory management, vector embeddings, deduplication, and retention policy enforcement for the Xavier agent ecosystem.

## Prune REST API (`POST /v1/memories/prune`)

The Prune API allows operators and background jobs to purge stale or unwanted memories according to retention rules.

### Parameters

- `kind` *(optional string)*: Filter memories by kind (e.g., `fact`, `decision`, `task`).
- `older_than_days` *(optional integer)*: Filter memories whose last access time (`last_accessed_at`, `updated_at`, or `created_at`) is older than the specified number of days.
- `path_prefix` *(optional string)*: Filter memories whose canonical path starts with the specified prefix (e.g., `logs/temp`).
- `dry_run` *(optional boolean, default: `true`)*: When `true`, matches records and returns candidate counts without performing physical deletions. Set to `false` to permanently delete matched memories.

### Example Request

```json
POST /v1/memories/prune
{
  "kind": "fact",
  "path_prefix": "logs/temp",
  "older_than_days": 7,
  "dry_run": false
}
```

## Store Deduplication & Consolidation Policy (`VecSqliteMemoryStore`)

Deduplication and consolidation policies govern how new memory insertions are merged with existing records in `sqlite_vec_store`:

- **Scopes (`DedupScope`)**:
  - `PathExact`: Deduplicates entries sharing the exact same `path` and `workspace_id`.
  - `Namespace`: Deduplicates entries matching organization, user, agent, session, project, and scope metadata.
- **Similarity Threshold**: Uses cosine similarity (`threshold` default: `0.85`) on vector embeddings to identify matching memories.
- **In-Place Superset Updates**: If new content is a strict superset of existing content, it replaces the record in-place without appending duplicate revisions.
- **Revision History**: If new content differs significantly, old content is archived into the record's `revisions` list (capped by `max_revisions`, default: `5`).
- **Canonical SSP Paths**: Paths under `features/` and `stability/` automatically operate in `PathExact` deduplication mode for deterministic state updates.
