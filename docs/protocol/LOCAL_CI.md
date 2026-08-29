# Local CI Parity — `scripts/ci-local.sh`

Xavier's CI runs **locally on the node** (the SWAL standard does not rely on
GitHub Actions billing). `scripts/ci-local.sh` executes the exact canonical
gate set that the repo standard (see `AGENTS.md`) requires, so a green local
run is parity with what any merge gate expects.

## What it runs (in order)

| # | Gate     | Command |
|---|----------|---------|
| 1 | `fmt`     | `cargo fmt --all --check` |
| 2 | `clippy`  | `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets --features ci-safe --exclude app` |
| 3 | `check`   | `RUSTFLAGS="-D warnings" cargo check --workspace --features ci-safe --exclude app --all-targets` |
| 4 | `test`    | `cargo test -p xavier --lib --features ci-safe` |
| 5 | `secrets` | `scripts/check-secrets.sh` |

Behavior:

- **Fail-fast**: the script exits non-zero on the first failing gate.
- **Summary table**: a per-gate PASS/FAIL/SKIP table with timings is always
  printed on exit (even on failure).
- **`ci-safe` feature** is used for clippy/check/test so CI never requires the
  local LLM toolchain; the Tauri `app` package is excluded (UI is validated in
  its own lane).

## Usage

```bash
# Full local CI run (all five gates):
scripts/ci-local.sh

# Run a single gate (useful when iterating on one failure):
scripts/ci-local.sh fmt
scripts/ci-local.sh clippy
scripts/ci-local.sh check
scripts/ci-local.sh test
scripts/ci-local.sh secrets
```

Exit codes: `0` all executed gates passed · `1` a gate failed ·
`2` usage error (unknown gate).

## RAM-disk builds (`CARGO_TARGET_DIR`)

The script honors `CARGO_TARGET_DIR` if exported (convention on this node:
builds go to tmpfs and avoid lock contention between parallel agents):

```bash
export CARGO_TARGET_DIR=/tmp/cargo-target
scripts/ci-local.sh
```

If it is not set, cargo's default `target/` directory is used and the script
prints a notice.

## When to run it

- Before every commit/PR (CI-equivalent signal in seconds-to-minutes).
- After merges to `main`, to catch `-D warnings` drift (e.g., unfulfilled
  `#[expect(dead_code)]`) the way `verify-pipeline.sh` catches feature-ledger
  drift.
- In hooks or cron: `scripts/ci-local.sh fmt` is cheap enough for a
  pre-commit gate; the full run belongs in pre-push.

Related: `scripts/verify-pipeline.sh` (feature ledger verification),
`docs/protocol/README.md` (wave protocol).
