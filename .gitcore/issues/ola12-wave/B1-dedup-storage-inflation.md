# B1: Fix dedup storage-inflation bug

## Problem

`revisioned_record()` in `src/memory/sqlite_vec_store/store_impl.rs` pushes a
FULL copy of the content to the `revisions` table on every merge. 10 dedup merges
= 10 complete copies of the content in revisions. This INFLATES storage instead
of reducing it.

## Solution

Smart revision policy:

1. If new content is a **superset** of old content → REPLACE in place, no revision push
2. If content **differs** (not superset) → push 1 revision with configurable cap
3. `max_revisions` (default 5) enforced at insert time

### Steps

1. Implement `is_superset(new: &str, old: &str) -> bool` helper (check if old is substring of new)
2. Modify `revisioned_record()` to check superset before pushing
3. Add `MAX_REVISIONS` enforcement: if revisions.len() >= max, evict oldest
4. Add tests for superset detection and revision cap
5. Verify storage reduction with a test: 10 sequential merges should produce ≤2 revisions

## Acceptance

- [ ] 10 sequential superset merges → 0 extra revisions
- [ ] 10 sequential different merges → max 5 revisions (cap)
- [ ] `cargo test -p xavier --lib dedup` passes
- [ ] No regression in existing dedup tests

## Files

- `src/memory/sqlite_vec_store/store_impl.rs`
- `src/memory/sqlite_vec_store/schema_impl.rs` (tests)
