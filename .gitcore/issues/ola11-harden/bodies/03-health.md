> Parent EPIC: {{EPIC}}

# [Ola 11 · 03] Fix health test_overall_status_prioritization

> Wave **Ola 11 — Harden Residuals**. Host `--lib` failure.

## Current State (MEDIBLE)
- Fail: `health::tests::test_overall_status_prioritization`
- Expected `status == "healthy"`, got `"warn"` on first `collect_health(&settings, None)`
- File: `src/health/mod.rs` (~1079 lines)

## Desired State (DELTA)
- Test isolates environment so baseline is deterministic OR asserts prioritization without requiring a perfectly healthy host
- Prioritization logic (critical fail → warn/degraded) remains correct and covered

## Web Research Required
1. Read `collect_health` + status aggregation in `src/health/mod.rs`
2. search: `health check unit test hermetic environment rust 2025`

## Exact Technical Context
- CRITICAL: ONLY `src/health/mod.rs`
- Do not edit `src/observability/health.rs` or `src/health/repair.rs`

## Problem
Host-dependent health baseline makes `--lib` fail on developer/CI machines that are not "perfectly healthy".

## Acceptance Criteria
- [ ] `cargo test -p xavier --lib health::tests::test_overall_status_prioritization` passes
- [ ] `cargo check -p xavier` 0 errors
- [ ] Only listed file changed

## Files to Modify
| File | Change | Risk |
|------|--------|------|
| `src/health/mod.rs` | Hermetic test / status assert | MED |

## DO NOT touch
- `src/health/repair.rs`, `src/observability/**`, features.json

## Verification
```bash
CARGO_TARGET_DIR=/tmp/rt cargo test -p xavier --lib health::tests::test_overall_status_prioritization -- --nocapture
```

## Dependencies & Merge Order
- **Expected effort:** Medium
