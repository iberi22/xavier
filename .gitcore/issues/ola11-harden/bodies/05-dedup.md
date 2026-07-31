> Parent EPIC: {{EPIC}}

# [Ola 11 · 05] Fix PathExact dedup threshold behavior (test_custom_dedup_policies)

> Wave **Ola 11 — Harden Residuals**. Host `--lib` failure.

## Current State (MEDIBLE)
- Fail: `memory::tests::tests::test_custom_dedup_policies` at assert `list.len() == 2` (got 1)
- Scenario: PathExact + threshold 0.95; second put has altered embedding (intended similarity < 0.95) but still deduped
- Production logic: `src/memory/sqlite_vec_store/store_impl.rs` (`set_dedup_settings` / PathExact match)
- Test: `src/memory/tests.rs`

## Desired State (DELTA)
- PathExact must still respect similarity threshold when embeddings differ below threshold (OR document intentional path-only dedup and fix the test accordingly — prefer correct product behavior: threshold applies)
- `test_custom_dedup_policies` passes

## Web Research Required
1. Read PathExact branch in `store_impl.rs` (~lines 119–285)
2. search: `cosine similarity threshold deduplication embeddings 2025`

## Exact Technical Context
- Primary file: `store_impl.rs`
- May adjust fixture embeddings in `src/memory/tests.rs` ONLY if production behavior is correct and fixture math wrong
- CRITICAL: Do NOT edit `schema_impl.rs` (issue 04)

## Problem
Dedup policies claim threshold support but PathExact collapses revisions incorrectly.

## Acceptance Criteria
- [ ] `cargo test -p xavier --lib memory::tests::tests::test_custom_dedup_policies` passes
- [ ] Diff limited to allowed files
- [ ] `cargo check -p xavier` 0 errors

## Files to Modify
| File | Change | Risk |
|------|--------|------|
| `src/memory/sqlite_vec_store/store_impl.rs` | Threshold/PathExact fix | HIGH |
| `src/memory/tests.rs` | Optional fixture tweak | LOW |

## DO NOT touch
- `schema_impl.rs`, `src/settings/**`, features.json

## Verification
```bash
CARGO_TARGET_DIR=/tmp/rt cargo test -p xavier --lib memory::tests::tests::test_custom_dedup_policies -- --nocapture
```

## Dependencies & Merge Order
- **Parallel with:** 04
- **Expected effort:** Medium
