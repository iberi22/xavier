# B3: Propagate reindex errors silently-but-verbosely

## Problem

`reindex_null_embeddings_background()` in `schema_impl.rs:271-277` swallows
errors from the vec insert with a single `tracing::error!`. When the insert
fails, the function returns `Ok(())` and downstream assertions fail with
confusing "has_vector_row" errors instead of the real cause.

## Solution

Improve error visibility without changing production behavior:

1. Add a counter for successful vs failed reindex operations
2. Log the specific failure reason per record (not just "Failed to update")
3. Return `Ok(count)` with the number of successfully reindexed records
4. Add `warn!` if any records failed, with the count

### Steps

1. Change return type from `Result<()>` to `Result<usize>` (success count)
2. Track `success_count` and `fail_count` in the loop
3. After loop, if fail_count > 0, log `warn!("Reindex: {success_count} OK, {fail_count} FAILED")`
4. Update callers to handle `Result<usize>` (there's only one: the background spawn at line 107)
5. Add test that verifies success count is returned correctly

## Acceptance

- [ ] Return type is `Result<usize>` with success count
- [ ] Failed inserts logged with record ID and error details
- [ ] Callers updated (only background spawn)
- [ ] Existing reindex test passes
- [ ] New test verifies success count

## Files

- `src/memory/sqlite_vec_store/schema_impl.rs`
