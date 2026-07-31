> Parent EPIC: {{EPIC}}

# [Ola 11 · 04] Fix reindex_null_embeddings_background test

> Wave **Ola 11 — Harden Residuals**. Host `--lib` failure.

## Current State (MEDIBLE)
- Fail: `memory::sqlite_vec_store::schema_impl::tests::test_reindex_null_embeddings_background`
- Panic: `assertion failed: has_vector_row` after reindex
- File: `src/memory/sqlite_vec_store/schema_impl.rs` (~690 lines)

## Desired State (DELTA)
- Background reindex writes embedding blob AND `memory_embeddings` row (or test matches real schema contract)
- Test uses isolated temp DB + mock embedding HTTP as already started in test setup

## Web Research Required
1. Read the failing test end-to-end in `schema_impl.rs`
2. Read `reindex_null_embeddings_background` implementation in same file
3. search: `sqlite-vec upsert embedding row rust 2025`

## Exact Technical Context
- CRITICAL: ONLY `src/memory/sqlite_vec_store/schema_impl.rs`
- Do NOT edit `store_impl.rs` (issue 05)

## Problem
Reindex path claims success but vector table row missing — breaks embedding recovery.

## Acceptance Criteria
- [ ] `cargo test -p xavier --lib memory::sqlite_vec_store::schema_impl::tests::test_reindex_null_embeddings_background` passes
- [ ] Only listed file changed

## Files to Modify
| File | Change | Risk |
|------|--------|------|
| `src/memory/sqlite_vec_store/schema_impl.rs` | Fix reindex write or test contract | MED |

## DO NOT touch
- `store_impl.rs`, `src/memory/tests.rs`, features.json

## Verification
```bash
CARGO_TARGET_DIR=/tmp/rt cargo test -p xavier --lib memory::sqlite_vec_store::schema_impl::tests::test_reindex_null_embeddings_background -- --nocapture
```

## Dependencies & Merge Order
- **Parallel with:** 05 (different file)
- **Expected effort:** Medium
