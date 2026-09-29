# [PAGEINDEX.04] SQLite tree store (WAL, workspace-scoped)

> Wave pageindex — F1 | Feature: `feat-pageindex-tree-core` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 01 | Parallel group: G1 parallel {02,03,04} | Risk: MED | Effort: Medium 3h
> Worktree/branch: dedicated `feat/pageindex-04` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- No store. Xavier core DB is in `src/storage/*` and MUST NOT be touched.

## Desired State (DELTA)

`store::{Store trait, SqliteStore}` with tables `pi_documents`, `pi_nodes`, `pi_pages`; `PRAGMA journal_mode=WAL; foreign_keys=ON`; `UNIQUE(workspace,name)`; idempotent upsert by `content_hash`; cascade delete; `get_pages(range)`; list/search-by-title; schema version table for future migrations. Sync API (rusqlite), callers use `spawn_blocking`.

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `crates/xavier-pageindex/src/store/mod.rs` | fill stub: trait | MED |
| `crates/xavier-pageindex/src/store/schema.rs` | fill stub: DDL + version | MED |
| `crates/xavier-pageindex/src/store/sqlite.rs` | fill stub | MED |
| `crates/xavier-pageindex/tests/store.rs` | new | MED |

## Tests that prove it

- `test_store_roundtrip_document_tree`
- `test_store_get_pages_by_range`
- `test_store_reingest_same_hash_is_idempotent`
- `test_store_delete_cascades_nodes_and_pages`
- `test_store_wal_and_foreign_keys_enabled`
- `test_store_workspace_isolation`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier-pageindex store_`
- [ ] `cargo clippy -p xavier-pageindex --all-targets -- -D warnings`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- `crates/xavier-pageindex/src/model.rs`; `src/storage/migrations.rs` (style only)
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- `src/storage/*`; never commit `*.sqlite3`
- `.gitcore/features.json` — status promoted only by a green `scripts/verify-pipeline.sh`, never by hand.
- `src/server/mcp/{mod,server,tools_core}.rs` unless this is issue 08 (V6 secrets wave owns them).
- `src/context/skill_controller/*`, `src/context/mod.rs` (another wave).
- Any file assigned to a parallel issue of this wave (keep file islands disjoint).

## Anti-Hallucination Guard

1. READ before write; verify every referenced symbol with grep.
2. No invented crates: new deps only as listed in the spec (sec. 4); keep default/`ci-safe` free of native libs.
3. New `std::env::var` => add key to `.env.example` in the same PR (read only in `pageindex_glue/settings.rs`).
4. Note: main CI has a pre-existing intermittent failure in "Rust Integration (Observability Contract)" (#2701); do not chase it here.

## PR Delivery Requirements

- [ ] 1 PR = this issue only; conventional commit `feat(pageindex): ...`; no co-author/AI trailers.
- [ ] `git diff --stat origin/main` is non-empty and matches the files table.
- [ ] If blocked, do not open an empty PR; comment the blocker on the issue.
