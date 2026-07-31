> Parent EPIC: {{EPIC}}

# [Ola 11 · 02] Fix offline_models test_get_offline_status flake/fail

> Wave **Ola 11 — Harden Residuals**. Host `--lib` failure.

## Current State (MEDIBLE)
- Fail: `cli::handlers::offline_models::tests::test_get_offline_status`
- Asserts `engine_status == "running"` but got `"stopped"` (host CI log 2026-07-31)
- File: `src/cli/handlers/offline_models.rs` (~377 lines)

## Desired State (DELTA)
- Test must not assume a global engine is running
- Either isolate with controlled fixture (env/temp config) OR assert the real stopped/running contract deterministically
- Other tests in the same module (`*_running`, `*_stopped`) already pass — align `test_get_offline_status` with that pattern

## Web Research Required
1. Read sibling tests in the same `#[cfg(test)]` module
2. search: `axum handler unit test isolate process state rust 2025`

## Exact Technical Context
- CRITICAL: ONLY `src/cli/handlers/offline_models.rs`
- Do not change production semantics unless the handler itself is wrong; prefer test isolation

## Problem
One flaky/wrong assertion blocks `cargo test --lib` green on host.

## Acceptance Criteria
- [ ] `cargo test -p xavier --lib cli::handlers::offline_models::tests::test_get_offline_status` passes
- [ ] Full offline_models test module passes
- [ ] Only listed file changed

## Files to Modify
| File | Change | Risk |
|------|--------|------|
| `src/cli/handlers/offline_models.rs` | Fix test (or tiny handler bug if proven) | LOW |

## DO NOT touch
- `src/settings/**`, `src/agents/**`, features.json

## Verification
```bash
CARGO_TARGET_DIR=/tmp/rt cargo test -p xavier --lib cli::handlers::offline_models -- --nocapture
```

## Dependencies & Merge Order
- **Parallel with:** all other 01–11 except none
- **Expected effort:** Small
