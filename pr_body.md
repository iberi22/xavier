## Description
Implements [WAVE-1012.01] verify-pipeline-diagnostics to surface the first build error and fail fast when the full-ledger warmup cannot compile.

### What Changed
* **Warmup diagnostics**: captures `cargo test ... --no-run` output and prints the first 40 compiler/linker error lines in a `::group::warmup <spec>` block instead of silencing them.
* **Fail-fast**: if the core `-p xavier --lib` warmup fails, immediately exits with 1, avoiding iterating 100+ failing tests.
* **`try_cargo` deduplication**: extracts the first `error` line from cargo output into a temporary tracker file. Distinct errors trigger printing the first 20 output lines.
* **New verify-pipeline-test.sh**: offline bash unit test using a mocked `cargo` and isolated `.gitcore/features.json` to verify the pipeline logic without running full Cargo builds.

### Testing & Verification
```
$ bash scripts/tests/verify-pipeline-test.sh
==========================================================
Test Case 1: Fail-fast on -p xavier --lib
PASS: Test Case 1
==========================================================
Test Case 2: Success path intact
PASS: Test Case 2
==========================================================
Test Case 3: try_cargo deduplication
PASS: Test Case 3
==========================================================
Test Case 4: check-only mode
PASS: Test Case 4
All verify-pipeline tests passed

$ bash scripts/verify-pipeline.sh --check-only
==> Xavier verify pipeline (mode: check, root: /app)
==> [1/5] Validating ledger schema...
  ✅ ledger OK: 100 features
==> [2/5] Checking implemented_in[] paths...
  ✅ all paths exist
==> [3/5] Computing implementation score...
  stable=74 beta=16 planned=10 total=100 real%=90.0

==> Pipeline complete. ✅
```

### Decisions
* Re-implemented warmup compilation using an intermediate temporary file instead of `/dev/null`.
* Used `grep -E '^error|error\[|could not compile|linker|signal'` for generic error pattern matching based on standard rustc output formats.
* Implemented the test using temporary `bin/cargo` mock scripts that return specific output/codes. `Cargo.toml` is also mocked in tests because the pipeline uses `grep` on it, and without an explicit `[[test]]` mock under `set -e`, the pipeline would crash early.
