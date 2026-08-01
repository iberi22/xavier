# A3: Cargo warnings cleanup

## Problem

`cargo check -p xavier --lib` produces 28 warnings. These include dead_code,
unfulfilled lint expectations, and unused imports. While not errors, they pollute
CI output and hide real issues.

## Solution

Run `cargo fix` and manually resolve remaining warnings.

### Steps

1. `cargo fix --lib -p xavier --tests --allow-dirty` (auto-fixes ~4 suggestions)
2. Manually fix remaining 24 warnings:
   - `dead_code` in `src/cli/state.rs` (lines 43, 55) — remove or add `#[allow]` with justification
   - Unfulfilled lint expectations — either use the lint or remove the expectation
   - Any remaining `unused` warnings
3. Verify: `cargo check -p xavier --lib 2>&1 | grep warning | wc -l` → 0

## Acceptance

- [ ] `cargo check -p xavier --lib` produces 0 warnings
- [ ] `cargo test -p xavier --lib` still passes (1323+ tests)
- [ ] No functional code changes (warnings only)

## Files

- ~15 source files (minor edits)
