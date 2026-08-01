# D1: Local CI pipeline

## Problem

GitHub Actions budget is exhausted. No local CI exists. Merges require
manual diff review + cargo check, which is error-prone and slow.

## Solution

Create a local CI script that mirrors GHA checks.

### Pipeline steps

1. `cargo check -p xavier --lib` (compilation)
2. `cargo test -p xavier --lib` (unit tests)
3. `cargo clippy -p xavier --all-targets` (lints)
4. `cargo fmt -p xavier -- --check` (formatting)
5. Report pass/fail summary

### Steps

1. Create `scripts/ci-local.sh` with the pipeline above
2. Add `--fix` flag option for auto-fixing fmt/clippy issues
3. Add `--quick` flag for check-only (skip tests)
4. Document usage in `docs/ops/local-ci-with-agent-priv.md` (already exists)
5. Test: run against current main, verify all checks pass

## Acceptance

- [ ] `scripts/ci-local.sh` runs all 4 checks
- [ ] Exit code 0 on success, non-zero on failure
- [ ] `--fix` flag auto-fixes fmt and clippy
- [ ] `--quick` flag runs only cargo check
- [ ] Script documented in ops docs

## Files

- `scripts/ci-local.sh` (new)
- `docs/ops/local-ci-with-agent-priv.md` (modify)
