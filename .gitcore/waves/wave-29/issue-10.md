# [WAVE-29.10] feat-e2e-observability-contract — E2E gate asserting MCP health agrees with GET /health

> Wave 29 — E2E + CI. Labels: `wave-29`, `ola29` (NO `jules` yet)
> Merge order: **10/10 — LAST**. Risk: MED | Effort: Medium 2-4h

---

## Current State (MEDIBLE)

Files: `tests/` (the integration/E2E test dir exists; `tests/e2e/` is present),
`.github/workflows/ci.yml` (230 lines)

**Defect 1 — there is no automated check that the reporting layer is truthful.**
Every defect in this wave was found by hand, by an agent comparing tool output against `systemctl`
and `curl`. Nothing in CI asserts the invariant. Concretely, all of the following were true
simultaneously on one running process and nothing failed:
- `xavier_health_check` (MCP) reported `embeddingOk: false`, `status: "degraded"`
- `GET /health` on the same process reported `embedding.status: "healthy"`, `available: true`,
  `database.integrity_ok: true`, `fts_ok: true`, `llm.reachable: true`, `mesh.status: "healthy"`
- `xavier_env_status` (MCP) reported `xavier.service: "inactive"`
- `systemctl --user is-active xavier.service` reported `active`
- `xavier health` printed `Version: unknown` while `xavier --version` printed `0.2.15`
- `xavier stats` returned 3 fields while the `xavier_stats` MCP tool returned 5 substantive ones

A regression test for "the health endpoint does not lie" does not exist. This issue creates it.

**Defect 2 — `tests/e2e` is not wired into CI.**
`.github/workflows/ci.yml` runs, for Rust changes:
- `rust-fmt` → `cargo fmt --all -- --check`
- `rust-clippy` → `cargo check --package xavier --all-targets --features ci-safe` and
  `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings`
- `rust-msrv` → `cargo check` on toolchain `1.94`
- `rust-test` → `cargo test --package xavier --lib --features ci-safe -- --test-threads=1`
  (**`--lib` only** — integration tests under `tests/` are NOT executed)
- `secret-scan` → `scripts/check-secrets.sh`
- `frontend-build-audit` (panel-ui, incl. Playwright) when frontend paths change
- `version-gate`

So nothing in `tests/` runs in CI. Also note the comment at ci.yml lines 145-153: tests are
pinned to `--test-threads=1` because several tests mutate global ENV and cannot be parallelised
across processes. Any new E2E job must respect that.

There is a known-broken E2E already tracked separately: issue #2507
`[ci] panel-ui Playwright e2e requires live backend :8006 (fails on any panel-ui PR)`.
Do not attempt to fix that here — it is a different subsystem (Playwright, panel-ui) and a
different owner.

## Desired State (DELTA)

- **New integration test file** under `tests/` (e.g. `tests/observability_contract.rs`) that
  boots the real HTTP server in-process (or binds an ephemeral port) and asserts the invariants
  that were violated:
  1. `GET /health` reports `embedding.status == "healthy"` **iff** the embedding probe reports
     available — and the MCP `health_check` tool's `embeddingOk` agrees with it. This is the
     exact regression that produced the false `degraded`.
  2. The top-level `status` is not `degraded` while every subsystem field is healthy, OR the
     response names the reason. If WAVE-29.01's `degraded_reasons` has landed, assert it is
     non-empty in that case; if not, skip that sub-assertion with a clear message (do not fail
     the build for a field that does not exist yet).
  3. `embedding_coverage` never reports `percent == 100.0` together with `total == 0`.
  4. The server-reported version equals `env!("CARGO_PKG_VERSION")` — the CLI/MCP parity that
     WAVE-29.07 fixes in the CLI. Assert the *server* side here; the CLI text is 29.07's job.
  5. The `xavier_stats` MCP tool and the stats HTTP endpoint report the same substantive
     counters (`total_memories`, `total_entities`, `storage_bytes`).
- **Wire `tests/` into CI**: add a job (e.g. `rust-integration`) that runs
  `cargo test --package xavier --tests --features ci-safe -- --test-threads=1`, gated by the same
  `changes.outputs.rust == 'true'` condition as the other Rust jobs, with
  `timeout-minutes` set generously (the lib suite already takes ~100s at 1 thread) and the
  `Swatinem/rust-cache@v2` cache step copied from `rust-test`.
- **The test must be hermetic**: no dependency on a live Ollama, a live `:8006`, or network.
  Use fakes/mocks and a temp data dir. It must pass on a GitHub runner with no services.
- **New tests**: the assertions above, plus a self-test that the suite degrades gracefully
  (skips, does not fail) when an optional field such as `degraded_reasons` is absent.

## 🌐 Web Research Required

**MANDATORY — 4 queries. Research BEFORE implementing.**
1. search: "Rust axum integration test onecpolar serve Router oneshot tower ServiceExt"
2. search: "GitHub Actions cargo test integration tests job matrix timeout-minutes"
3. search: "hermetic test no external service dependency ephemeral port bind port 0"
4. search: "conditional assertion skip when feature absent Rust test"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read these files COMPLETELY before editing:
   - `.github/workflows/ci.yml` (all 230 lines) — copy the structure, the `changes` gating, the
     toolchain setup, the cache step and the swap-setup step from the existing `rust-test` job.
   - the existing `tests/` directory: list it, then read the files that already boot a server or
     a store. FOLLOW THEIR PATTERNS — do not invent a new harness.
   - `src/health/mod.rs` and `src/server/mcp/tools_core.rs` to know the exact response shapes
     you assert on. READ ONLY — both are other issues' islands.
3. Write the test FIRST and watch it FAIL against current `origin/main` for at least assertions
   1 and 3 (those are the confirmed regressions). Then keep it green only if the corresponding
   fix has already merged; otherwise mark it `#[ignore]` WITH a comment naming the issue that
   must land, and say so in the PR body.
4. Execute in 3 micro-phases:
   - **Phase 1 — Types & Contracts**: the test harness (server bootstrap) reusing existing
     patterns; the assertion helpers.
   - **Phase 2 — Core Logic**: the assertions; the CI job.
   - **Phase 3 — Tests & Delivery**: run the full verification block, open the PR.
5. **Blocker Recovery**: on any failure, search the web for the exact error signature, fix it,
   document it in the PR body. NEVER wait for feedback."

## Existing Code Patterns (MUST follow)

- Copy the `rust-test` job structure in `ci.yml` verbatim where possible (checkout v7,
  `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`, `timeout-minutes: 45`).
- Gate on `needs: changes` + `if: needs.changes.outputs.rust == 'true'` like every other Rust job.
- Keep `--test-threads=1` on every cargo test invocation (see the ci.yml comment at lines 145-153
  for the full rationale — global ENV mutation across processes).
- Reuse the existing test bootstrap in `tests/`; do not build a parallel one.
- Comments in English, minimal density.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep -c "^error"` == 0
- [ ] `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep -c "^error"` == 0
- [ ] `cargo test --package xavier --tests --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` produces NO output
- [ ] `cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` produces NO output
- [ ] `cargo fmt --all -- --check` produces no diff
- [ ] `grep -n "rust-integration\|name: Rust Integration" .github/workflows/ci.yml` >= 1 match
- [ ] `grep -n "tests --features ci-safe" .github/workflows/ci.yml` >= 1 match
- [ ] `grep -c "degraded_reasons" tests/observability_contract.rs` >= 1 (present but tolerant)
- [ ] `grep -n "8006\|localhost" tests/observability_contract.rs` returns NO hardcoded live-port dependency (or is confined to a documented skip)
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 2 (test file + ci.yml)

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `tests/observability_contract.rs` (new) | does not exist | E2E contract assertions, hermetic | MED |
| `.github/workflows/ci.yml` | 230 lines, no `tests/` execution | Add `rust-integration` job | MED |

You may add more than one file under `tests/` if the existing layout requires it, but **nothing
outside `tests/` and `.github/workflows/ci.yml`** may be modified.

## DO NOT touch (Anti-Regression)

- `src/health/mod.rs` — island of WAVE-29.01. You assert on its output; do NOT change it to make
  your test pass. If the test fails because 29.01 has not merged, that is expected — see step 3.
- `src/server/mcp/tools_core.rs`, `types.rs`, `tests.rs`, `tools_context.rs` — islands of
  WAVE-29.02 / 29.10's assertions. READ ONLY.
- `src/self_manage/mod.rs` — island of WAVE-29.03.
- `src/context/**` — island of WAVE-29.04.
- `src/cli/**`, `src/auto_improvement/**`, `code-graph/**`, `src/memory/**` — other islands.
- **Do NOT touch the other workflows.** `.github/workflows/` contains `coverage.yml`,
  `deploy-docs.yml`, `jules-ci-feedback.yml`, `pr-gate.yml`, `release.yml`,
  `rust-coverage.yml`. Only `ci.yml` is yours. Parallel PRs commonly conflict here — if a
  conflict appears, rebase; do not merge someone else's workflow changes into your diff.
- **Do NOT attempt to fix issue #2507** (panel-ui Playwright needs a live backend). Out of scope.
- `.gitcore/features.json`, `Cargo.toml`, `Cargo.lock`, `Cargo.lock` must not change.
- Do NOT add a service container, Ollama, or any external dependency to CI.

## Anti-Hallucination Guard ⚠️

1. **READ before write.** Follow the existing bootstrap in `tests/`. If no server bootstrap
   exists, `src/server/mcp/tests.rs` has one — study it before writing a new harness.
2. **Do not weaken the assertions to make them pass.** The point of this issue is to catch the
   lies. If an assertion fails because the corresponding fix has not merged, mark it `#[ignore]`
   with a comment naming the owning issue — do NOT delete it and do NOT loosen it.
3. **Do not require Ollama, network, or a live :8006.** The test must pass on a bare GitHub
   runner. If you need the embedder, fake it.
4. **Do not change the tool count or the MCP tool surface.**
5. **Do not edit the other workflow files.** Only `ci.yml`.
6. **`cargo check` gate**: 0 errors before commit.

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows `tests/observability_contract.rs` (new) and
      `.github/workflows/ci.yml` (modified) BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty
- [ ] `git show HEAD --name-only | grep -cE "tests/|\.github/workflows/ci.yml"` >= 2
- [ ] `wc -l tests/observability_contract.rs` >= 60 (a real test file, not a stub)
- [ ] `grep -c "#\[test\]\|#\[tokio::test\]" tests/observability_contract.rs` >= 4
- [ ] PR body shows the test output BEFORE the other 9 issues merged (the failures it catches)
- [ ] PR body lists, per assertion, which wave issue makes it pass — and marks any that are
      still `#[ignore]` with the owning issue number
- [ ] PR body includes the exact `cargo test --package xavier --tests` output
- [ ] If the work could not be completed: DO NOT open a PR — comment the blocker on this issue.

## Verification

```bash
cargo fmt --all -- --check
cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l   # expect 0
cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --package xavier --tests --features ci-safe -- --test-threads=1 2>&1 | tail -30
cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | tail -10
```

## Dependencies & Merge Order

- **Depends on:** 29.01, 29.02, 29.03, 29.04, 29.05, 29.06, 29.07, 29.08, 29.09.
  **This is merge order 10/10 and MUST be rebased onto `main` after the other nine land.**
  Its assertions encode the contracts those issues establish.
- **Parallel with:** nothing at write time — start it, but expect a rebase before merge.
- **Expected effort:** Medium 2-4h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix errors, do NOT commit broken code |
| No existing server bootstrap exists in `tests/` | Model it on `src/server/mcp/tests.rs`; if that is impractical, bind the router with `tower::ServiceExt::oneshot` and skip the network entirely |
| Test requires Ollama and fails in CI | You violated hermeticity. Fake the embedder; do not add a service |
| Assertions fail because 29.01-29.09 have not merged | Expected. `#[ignore]` + a comment naming the owning issue, and report it in the PR body |
| `ci.yml` conflicts with a parallel PR | `git pull --rebase origin main`, resolve, re-run verification. Never force-push |
| The new job is too slow for CI limits | Raise `timeout-minutes`; do NOT raise the thread count (see ci.yml lines 145-153) |
| You believe `pr-gate.yml` must change too | Do NOT change it. Comment on this issue describing what it would need |
