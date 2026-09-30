# [WAVE-29.06] feat-improve-bounded — `xavier improve` prints a banner then never returns

> Wave 29 — CLI reliability. Labels: `wave-29`, `ola29` (NO `jules` yet)
> Merge order: 6/10 | Risk: LOW | Effort: Small 1-2h

---

## Current State (MEDIBLE)

File: `src/cli/commands/improve.rs` (banner at line 45); the cycle it drives lives in
`src/auto_improvement/` (`cycle.rs`, `gaps.rs`, `benchmark.rs`)

Measured, on the live host:
```
$ timeout 45 xavier improve
Running auto-improvement cycle (autonomous=false, ci=false)...
rc=124                       # killed by timeout, never finished
$ timeout 100 xavier improve 2>/dev/null
rc=124
```
The command prints exactly one line and then blocks indefinitely. The auto-improvement loop is
therefore **completely inoperable** — it is the mechanism the project relies on to detect and
close its own gaps.

Code shape: line 45 emits
`"Running auto-improvement cycle (autonomous={}, ci={})..."` and then the cycle proceeds with no
overall deadline.

Related, measured, from the same subsystem: the `sys_health` MCP tool builds a
`BenchmarkSnapshot` (in `src/server/mcp/tools_core.rs`, island of WAVE-29.02) with
`recall_at_k: 0.0`, `precision: 0.0`, `total_documents: 0`, `test_iterations: 0` hardcoded, and
feeds it to `crate::auto_improvement::gaps::analyze_gaps`. So even the inputs the gap analyzer
receives are placeholders. **WAVE-29.02 owns the call site in `tools_core.rs`; you own the
`auto_improvement` module itself. Do not edit `tools_core.rs`.**

This is the second of two hanging commands (the other is `doctor`, WAVE-29.05).

## Desired State (DELTA)

- **Bound the whole cycle** with a documented, configurable overall deadline. On expiry, the
  command must print what it managed to do, name the stage that did not finish, and exit
  non-zero. It must never block forever.
- **Bound each internal stage** where a stage can perform unbounded work (benchmark execution,
  experiment application, history persistence). Follow the same pattern as WAVE-29.05 —
  name the constants — but keep the two files disjoint.
- **Partial progress must be durable.** If stage 3 of 5 times out, stages 1-2 results must be
  persisted (the module already has `load_history` / an improvement-history file) rather than
  discarded, so the next run can resume.
- **Meaningful exit code**: 0 only when the cycle completed. Distinct non-zero when it was
  truncated by a deadline, distinct non-zero when it genuinely failed. Document the mapping.
- **Progress output**: the user currently sees one banner line and then nothing. Emit at least a
  line per stage so a human can see where it is. Keep the existing banner text intact.
- **New tests** in `src/cli/commands/improve.rs` or the `cycle` module it drives:
  - `improve_cycle_respects_overall_deadline`
  - `improve_persists_progress_when_stage_times_out`
  - `improve_exit_code_distinguishes_truncated_from_failed`
- Risk: LOW — but read `cycle.rs` fully first; the state machine may have more entry points than
  the CLI.

## 🌐 Web Research Required

**MANDATORY — 4 queries. Research BEFORE implementing.**
1. search: "tokio::time::timeout overall deadline graceful degradation partial results"
2. search: "checkpoint resume long running job stage persistence design"
3. search: "Rust CLI exit codes convention 0 success 1 failure 2 usage timeout"
4. search: "cancellation token tokio select loop stage"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read these files COMPLETELY before editing:
   - `src/cli/commands/improve.rs` (all of it)
   - `src/auto_improvement/cycle.rs` — the state machine, its stages, its history persistence
     (`load_history`, the improvement-history file)
   - `src/auto_improvement/gaps.rs` and `benchmark.rs` — the shapes this cycle consumes
     (`BenchmarkSnapshot`, `analyze_gaps`). Note `gaps.rs:190` also calls
     `crate::self_manage::log_scan` — that function is being fixed in WAVE-29.03; do not edit
     it, and make sure your change still compiles against its current signature.
3. Write the failing test FIRST: assert the cycle terminates under a short deadline.
   Then fix.
4. Execute in 3 micro-phases:
   - **Phase 1 — Types & Contracts**: deadline constants; a stage-outcome type that can express
     `Completed` / `Truncated` / `Failed`.
   - **Phase 2 — Core Logic**: per-stage and overall deadlines, durable partial progress,
     per-stage progress output, exit-code mapping.
   - **Phase 3 — Tests & Delivery**: 3 named tests, verification, PR.
5. **Blocker Recovery**: on any failure, search the web for the exact error signature, fix it,
   document it in the PR body. NEVER wait for feedback."

## Existing Code Patterns (MUST follow)

- The banner at line 45 uses `println!`; keep its exact wording.
- `load_history` / the improvement-history JSON is the established persistence pattern
  (`src/auto_improvement/cycle.rs`) — reuse it, do not invent a second store.
- `BenchmarkSnapshot` and `analyze_gaps` are the established data shapes; do not change their
  field types (WAVE-29.02 depends on them).
- `autonomous` and `ci` flags already exist — respect them; do not change their meaning.
- Comments in English, minimal density.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep -c "^error"` == 0
- [ ] `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error"` == 0
- [ ] `cargo test --package xavier --lib --features ci-safe improve -- --test-threads=1 2>&1 | grep "test result"` shows 0 failed
- [ ] `cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` produces NO output
- [ ] `grep -n "tokio::time::timeout" src/auto_improvement/cycle.rs src/cli/commands/improve.rs` >= 1 match
- [ ] `grep -n "IMPROVE_TOTAL_TIMEOUT\|IMPROVE_STAGE_TIMEOUT" src/auto_improvement/cycle.rs src/cli/commands/improve.rs` >= 2 matches
- [ ] `git diff --stat HEAD -- src/server/mcp/tools_core.rs` is EMPTY
- [ ] `cargo fmt --all -- --check` produces no diff
- [ ] PR body states the measured wall-clock of `improve` before and after
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/cli/commands/improve.rs` | banner at line 45, then unbounded | Deadlines, per-stage progress, exit codes, tests | LOW |
| `src/auto_improvement/cycle.rs` | state machine + history persistence | Stage outcomes, bounded stages, durable partial progress | MED |

## DO NOT touch (Anti-Regression)

- `src/server/mcp/tools_core.rs` — **island of WAVE-29.02**. It builds a `BenchmarkSnapshot` and
  calls `analyze_gaps`; you must keep those signatures compiling, but you do not edit that file.
- `src/self_manage/mod.rs` — island of WAVE-29.03. `gaps.rs:190` calls its `log_scan`; do not edit.
- `src/auto_improvement/gaps.rs`, `benchmark.rs` — only if strictly required to add a
  non-breaking field; prefer changing `cycle.rs` alone. If you must touch them, say why.
- `src/health/mod.rs`, `src/server/mcp/types.rs`, `src/context/**`, `src/memory/**` — other islands.
- `src/cli/handlers/doctor.rs` — island of WAVE-29.05 (the other hanging command).
- `src/cli/handlers/system.rs` — island of WAVE-29.07.
- `.gitcore/features.json`, `Cargo.toml`, `Cargo.lock`, `tests/**`, `.github/workflows/**`.

## Anti-Hallucination Guard ⚠️

1. **READ before write**: read `cycle.rs` fully. Check for other callers/entry points before
   changing a function signature.
2. **Do not "fix" the hang by deleting a stage.** Every stage must still run; they just get a
   deadline. A removed stage is a silent capability loss.
3. **Do not change `BenchmarkSnapshot` / `analyze_gaps` field types** — WAVE-29.02 is landing
   changes there in parallel and a type change will cause a conflict or a break.
4. **Do not add dependencies.** `tokio` is already present; verify the `time` feature.
5. **`cargo check` gate**: 0 errors before commit.

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows the modified files BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty
- [ ] `git show HEAD --name-only | grep -cE "improve|auto_improvement"` >= 1
- [ ] `grep -c "#\[test\]\|#\[tokio::test\]"` across your modified files shows 3 new tests
      (report before/after counts)
- [ ] PR body documents the exit-code mapping you chose in a small table
- [ ] PR body lists every stage now bounded and with what budget
- [ ] If the work could not be completed: DO NOT open a PR — comment the blocker on this issue.

## Verification

```bash
cargo fmt --all -- --check
cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l   # expect 0
cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | tail -20
```

## Dependencies & Merge Order

- **Depends on:** nothing. Merge order **6/10**.
- **Parallel with:** 29.01, 29.02, 29.03, 29.04, 29.05, 29.07, 29.08, 29.09, 29.10.
- **Expected effort:** Small 1-2h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix errors, do NOT commit broken code |
| Breaking a `BenchmarkSnapshot` field breaks `tools_core.rs` | Revert the field change; achieve the fix inside `cycle.rs` only |
| `gaps.rs:190` `log_scan` signature conflicts with WAVE-29.03 | Neither file is yours. Do not adapt the call — leave it and note the interaction in the PR body |
| `cycle.rs` has no natural stage boundaries | Add them at the existing function boundaries; do not restructure the algorithm |
| The cycle is genuinely fast on a healthy host | Still add the deadline — the point is that it must be bounded when the host is degraded. Test with an injected slow stage |
