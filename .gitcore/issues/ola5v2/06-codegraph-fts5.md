# [Ola 5v2 · 06] code-graph FTS5: replace symbols LIKE table scan

> **Re-launch** of #445. Skill: jules-async-orchestration gold standard.

## Web Research Required (Jules must search the web)

1. **SQLite FTS5 external content tables** — search: `sqlite FTS5 content content_rowid triggers 2024 2025`, read https://www.sqlite.org/fts5.html (external content tables + triggers).
2. **BM25 ranking with FTS5** — search: `sqlite fts5 bm25 rank ORDER BY bm25(table) 2024`, note function signature.
3. **rusqlite + FTS5** — search: `rusqlite fts5 virtual table rust example 2024 2025`, check whether `code-graph/Cargo.toml` needs features.

Paste 2–3 research bullets in the PR body.

## Exact Technical Context

- **File**: `code-graph/src/db/mod.rs` (~1048 lines)
- **`find_symbols`** ~412–425:

```sql
SELECT id, stable_id, name, kind, lang, file_path, start_line, end_line, start_col, end_col, signature, parent, complexity
FROM symbols
WHERE name LIKE ?1
```

- **`init_schema`** ~139
- **`insert_symbol` / `insert_symbols`** ~270 / ~319 — must keep FTS synchronized

Suggested schema (adapt if `id` type differs):

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
  name, kind, signature, file_path,
  content='symbols',
  content_rowid='id'
);
-- plus triggers after insert/delete on symbols OR explicit writes in insert_symbol
```

Query target:

```sql
SELECT s.* FROM symbols s
JOIN symbols_fts f ON s.id = f.rowid
WHERE symbols_fts MATCH ?1
ORDER BY bm25(symbols_fts)
LIMIT ?2
```

Empty query: return empty or fall back to LIKE with documented behavior + test.

> CRITICAL: **Only `code-graph/` crate.** DO NOT touch main `src/server/mcp`. DO NOT touch xavier-core/. NEVER loose `.patch` files. Empty PR rejected.

## Problem

`LIKE %q%` forces full table scans; unusable at 50k+ symbols.

## Acceptance Criteria

- [ ] FTS5 virtual table created in schema init
- [ ] Inserts keep FTS in sync (triggers or explicit)
- [ ] `find_symbols` uses MATCH + bm25 for non-empty queries
- [ ] Unit tests: insert symbols, find by name token
- [ ] `cargo test -p code-graph` passes
- [ ] `cargo check --workspace` 0 errors

## Files to Modify

| File | Change |
|---|---|
| `code-graph/src/db/mod.rs` | schema, insert, find_symbols |
| `#[cfg(test)]` in same module or tests | FTS tests |

**DO NOT touch:** `src/**` (xavier bin/lib), panel-ui/, xavier-core/

## Verification

```bash
cargo check --workspace
cargo test -p code-graph
```

## Dependencies and Merge Order

- **Depends on:** nothing
- **Can run in parallel with:** all non-code-graph issues
