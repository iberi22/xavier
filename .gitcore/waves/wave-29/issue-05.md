# [WAVE-29.05] feat-doctor-bounded — `xavier doctor` hangs forever and prints nothing to stdout

> Wave 29 — CLI reliability. Labels: `wave-29`, `ola29` (NO `jules` yet)
> Merge order: 5/10 | Risk: LOW | Effort: Small 1-2h

---

## Current State (MEDIBLE)

File: `src/cli/handlers/doctor.rs` (`handle_doctor` at line 746; `system_scan` probes in
`src/cli/handlers/system_scan.rs`)

Measured, on the live host, twice:
```
$ timeout 45  xavier doctor ; echo "rc=$?"
rc=124                       # 124 = killed by timeout
$ timeout 100 xavier doctor ; echo "rc=$?"
rc=124
stdout_bytes=0   stdout_lines=0
```
`xavier doctor` **never terminates** and writes **zero bytes to stdout**. The only output is
tracing logs on stderr:
```
system_scan: ollama 28.004463ms
system_scan: cli_agents 83.76711ms
system_scan: gpu 468.209µs
system_scan: docker 223.050732ms
system_scan: env_vars 33.173µs
```
Note the last probe logged is `env_vars`; the per-probe timings stop there, which is consistent
with a later probe blocking indefinitely.

`handle_doctor` (lines 746-758) runs seven check groups with **no overall timeout**:
```rust
let scan = scan_system(false).await;
checks.extend(check_database(&settings));
checks.extend(check_embeddings(&settings, &scan).await);   // network probe
checks.extend(check_memory(&settings, verbose));
checks.extend(check_mesh(&settings));
checks.extend(check_http(&settings, &scan).await);          // network probe
checks.extend(check_security(&settings));
checks.extend(check_scheduler(&settings));
```
The `.await` network probes (`check_embeddings`, `check_http`) have no `tokio::time::timeout`
wrapper. The final rendering (lines 773-776, `match format.as_str()` → json / markdown / table)
is therefore **unreachable** in practice — which is exactly why stdout is 0 bytes.

This is the flagship diagnostic command of the project. It is completely inoperable, and an
operator has no way to discover *which* probe is stuck.

Context (host pressure at the time of measurement): load average 101-114, swap 76.7% used,
PSI `io.some` avg10 41.7%. A resource-starved host makes the unbounded probes far more likely to
stall, so the missing timeout is a real production failure mode, not a theoretical one.

## Desired State (DELTA)

- **Wrap every individual probe in a bounded timeout.** Each check group must have a documented
  per-check budget (name the constant). A probe that exceeds it yields a `Warn` (or `Fail`)
  check entry saying "timed out after Ns" — it must NOT hang the command.
  Use `tokio::time::timeout`; the probes are already `async`.
- **Give the whole command an overall deadline** as a second safety net, so even a non-probe
  blocking section cannot hang forever.
- **Always render output.** Even when probes time out, the collected checks must be printed in
  the requested format (`json` / `markdown` / table). Zero bytes on stdout is never acceptable.
- **Exit code must be meaningful**: non-zero when `overall_status` is `Fail` (today the function
  returns `Result<()>` and the overall status is computed at lines 760-766 but appears not to
  influence the process exit code — verify and fix).
- **Report which probe hung.** A `timed_out: true` marker plus the probe name is required so the
  operator can act.
- **New tests** in this file:
  - `doctor_completes_when_a_probe_hangs`
  - `doctor_prints_output_even_with_timed_out_checks`
  - `doctor_exit_code_reflects_overall_status`
- Risk: LOW — additive, self-contained. `handle_doctor` is called from the CLI command layer;
  confirm the caller still compiles.

## 🌐 Web Research Required

**MANDATORY — 4 queries. Research BEFORE implementing.**
1. search: "tokio::time::timeout per-operation budget best practice async"
2. search: "CLI diagnostic command exit code convention degraded partial results"
3. search: "Rust anyhow context timeout error variant design"
3. search: "clap exit code non-zero on health check failure"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read these files COMPLETELY before editing:
   - `src/cli/handlers/doctor.rs` — especially `handle_doctor` (746-776), `DoctorReport`,
     `print_table_output`, `format_as_markdown`, and the individual `check_*` functions.
   - `src/cli/handlers/system_scan.rs` — the `scan_system` probes and their timeouts
     (the `system_scan: <probe> <elapsed>` log lines are around lines 80-101).
   - the CLI command wiring that invokes `handle_doctor` (find it with grep) to confirm the
     exit-code path.
3. Write the failing test FIRST: a test where one probe never returns, asserting `handle_doctor`
   still completes and still prints. Then fix.
4. Execute in 3 micro-phases:
   - **Phase 1 — Types & Contracts**: per-check timeout constants; a timeout marker on the check
     struct (additive field with a serde default).
   - **Phase 2 — Core Logic**: wrap each probe; add the overall deadline; guarantee rendering.
   - **Phase 3 — Tests & Delivery**: add the 3 named tests, run verification, open the PR.
5. **Blocker Recovery**: on any failure, search the web for the exact error signature, fix it,
   document it in the PR body. NEVER wait for feedback."

## Existing Code Patterns (MUST follow)

- `CheckStatus` (`Ok` / `Warn` / `Fail`) is the established verdict enum — reuse it, do not
  invent a parallel one.
- `scan_system(false).await` returns a scan struct consumed by `check_embeddings` and
  `check_http`; keep that data flow intact.
- Output dispatch at lines 773-776 uses `match format.as_str()` over `"json"`, `"markdown"`,
  `_` — preserve all three.
- `tracing::info!("system_scan: {} {:?}")` is the existing probe-timing log; keep it.
- Comments in English, minimal density.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep -c "^error"` == 0
- [ ] `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep -c "^error"` == 0
- [ ] `cargo test --package xavier --lib --features ci-safe doctor -- --test-threads=1 2>&1 | grep "test result"` shows 0 failed
- [ ] `cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` produces NO output
- [ ] `grep -n "tokio::time::timeout" src/cli/handlers/doctor.rs` >= 1 match
- [ ] `grep -n "DOCTOR_CHECK_TIMEOUT\|DOCTOR_TOTAL_TIMEOUT" src/cli/handlers/doctor.rs` >= 2 matches
- [ ] `cargo fmt --all -- --check` produces no diff
- [ ] PR body states the measured wall-clock of `doctor` before and after (before: >100s, never
      terminating; after: < 30s, measured)
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/cli/handlers/doctor.rs` | `handle_doctor` 746-776, 7 unbounded check groups | Per-check + overall timeouts, guaranteed rendering, exit code, 3 tests | LOW |
| `src/cli/handlers/system_scan.rs` | probe timings at 80-101 | Only if a probe itself needs a bound; otherwise leave untouched | LOW |

## DO NOT touch (Anti-Regression)

- `src/health/mod.rs`, `src/server/mcp/tools_core.rs`, `src/self_manage/mod.rs` — other islands.
- `src/context/skill_dispatcher.rs`, `src/context/skill_registry.rs` — island of WAVE-29.04.
- `src/cli/commands/improve.rs` — island of WAVE-29.06 (a different command that also hangs).
- `src/cli/handlers/system.rs` — island of WAVE-29.07 (`xavier health` / `xavier stats`).
- `src/memory/**` — island of WAVE-29.09.
- `.gitcore/features.json`, `Cargo.toml`, `Cargo.lock`, `tests/**`, `.github/workflows/**`.
- Do NOT change the `doctor` command name, its flags, or its output formats.

## Anti-Hallucination Guard ⚠️

1. **READ before write**: read `doctor.rs` fully. `handle_doctor` is `async`; the check functions
   have mixed sync/async signatures — do not assume.
2. **Do not guess which probe hangs.** Add bounds to ALL of them. Do not delete a probe to "fix"
   the hang — a missing probe is a silent capability loss.
3. **Do not add dependencies** for timing; `tokio` is already a workspace dependency. Verify the
   `tokio` features enabled in `Cargo.toml` include `time` before using `tokio::time::timeout`.
4. **Do not remove output formats.** All three must keep working.
5. **`cargo check` gate**: 0 errors before commit.

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows `src/cli/handlers/doctor.rs` modified BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty
- [ ] `git show HEAD --name-only | grep -c "src/cli/handlers/"` >= 1
- [ ] `grep -c "#\[test\]\|#\[tokio::test\]" src/cli/handlers/doctor.rs` shows 3 new tests
      (report before/after counts)
- [ ] PR body shows the BEFORE/AFTER of `handle_doctor`
- [ ] PR body lists every probe that is now bounded and with what budget
- [ ] If the work could not be completed: DO NOT open a PR — comment the blocker on this issue.

## Verification

```bash
cargo fmt --all -- --check
cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l   # expect 0
cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | tail -20
```

## Dependencies & Merge Order

- **Depends on:** nothing. Merge order **5/10**.
- **Parallel with:** 29.01, 29.02, 29.03, 29.04, 29.06, 29.07, 29.08, 29.09, 29.10.
- **Expected effort:** Small 1-2h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix errors, do NOT commit broken code |
| `tokio::time::timeout` unavailable | Check `Cargo.toml` tokio features; enabling an existing feature on `tokio` is acceptable, adding a new crate is not |
| A `check_*` function is sync and cannot time out | Wrap the call in `tokio::task::spawn_blocking` with a timeout, or bound its inner I/O; document which you chose |
| Exit-code change breaks a CI script or test | Find the assertion with grep, update it, and justify the new behaviour in the PR body |
| Test cannot simulate a hanging probe | Use a `#[tokio::test]` with a short timeout constant and an injected future; do not sleep for real seconds |
