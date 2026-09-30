# [WAVE-29.01] feat-health-truth — Health report must not fabricate healthy state

> Wave 29 — Observability honesty. Labels: `wave-29`, `ola29` (NO `jules` yet)
> Merge order: 1/10 | Risk: MED | Effort: Medium 2-4h

---

## Current State (MEDIBLE)

File: `src/health/mod.rs` (1140+ lines)

**Defect 1 — vacuous truth in the default constructor (line 38-47):**
```rust
impl Default for EmbeddingCoverage {
    fn default() -> Self {
        Self { indexed: 0, total: 0, percent: 100.0, status: "healthy".to_string() }
    }
}
```
`EmbeddingCoverage` is declared at line 31-36 with fields `indexed: u64, total: u64, percent: f64, status: String`.
`EmbeddingCoverage::default()` is consumed at **line 272** and **line 377** (the empty/fallback health constructors).

Measured production consequence: `GET /health` returned
`embedding_coverage: {indexed: 0, total: 0, percent: 100.0, status: "healthy"}` on a host with
1482 memories. **100% coverage of zero documents is reported as "healthy".** An agent that trusts
this field concludes embeddings are fully indexed when nothing is indexed at all.

**Defect 2 — `connected` defaults to false and drives global status:**
`connected: false` appears at **line 255** and **line 359** (fallback constructors).
The real probe result is destructured at **line 615**:
`let (connected, latency_ms, error_rate_pct, last_success, fallback_success) = ...`
`connected` is consumed at line 823 (`else if !embedding.connected || ...  { "degraded" }`),
line 877 (alert emission) and line 1096 (`let unhealthy = !embedding.connected ...`).

**Defect 3 — `degraded` is a single undifferentiated string.**
`"degraded"` is produced at lines 558, 815, 824, 831 and 946 with no field recording *why*.
Measured: every subsystem reported healthy (`database.integrity_ok:true`, `fts_ok:true`,
`llm.reachable:true`, `mesh.status:"healthy"`, `embedding.status:"healthy"`) yet the top-level
`status` was `"degraded"` solely because of host memory pressure. Nothing in the payload
distinguishes "my database is broken" from "the machine is out of RAM".

Tests already in this file (do not delete, they must keep passing):
`sys_health_snapshot_has_expected_fields` (308), `sys_health_alerts_fire_on_high_swap` (337),
`sys_health_alerts_fire_on_high_psi_io` (354), `sys_health_no_false_alerts_on_healthy_host` (370).

## Desired State (DELTA)

- **Line 38-47**: `Default for EmbeddingCoverage` must NOT claim health over an empty set.
  When `total == 0`, `percent` and `status` must express "no data", not "perfect".
  Use `percent: 0.0, status: "unknown"` and document the semantic in a doc comment on the struct
  (line 31) stating that `total == 0` means unmeasured, never perfect.
- **After line 615**: populate the real probe values into the returned struct. `provider` and
  `last_success` must not stay empty when the probe succeeded.
- **Lines 558, 815, 824, 831, 946**: add a `degraded_reasons: Vec<String>` (or an enum with a
  `Display`) to the health response struct declared near line 49-66, populated at each site with
  the subsystem that caused the degradation. Distinguish host-resource pressure from a
  subsystem failure. Keep the existing `status: String` field and its exact values
  (`healthy` / `degraded` / `unhealthy` / `critical`) so no consumer breaks.
- **New tests** in the existing `#[cfg(test)]` module of this same file:
  - `embedding_coverage_default_is_not_healthy_when_empty`
  - `degraded_reasons_distinguishes_host_pressure_from_subsystem_failure`
  - `degraded_reasons_empty_when_status_healthy`
- Risk: MED — `status` is consumed by `src/server/mcp/tools_core.rs:645` and by HTTP routes.
  Adding a field is additive and safe; changing the *values* of `status` is NOT allowed.

## 🌐 Web Research Required

**MANDATORY — 4 queries. Research BEFORE implementing.**
1. search: "Rust serde skip_serializing_if optional field additive API change"
2. search: "vacuous truth monitoring health check no data vs healthy best practice"
3. search: "Rust enum vs String status field backward compatibility refactor"
4. search: "Rust unit test assert custom message pattern 2026"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read `src/health/mod.rs` COMPLETELY (it is ~1140 lines; read it in segments with offset/limit).
   Pay special attention to: the `EmbeddingCoverage` struct (line 31), its `Default` impl
   (line 38), the unified health response struct (line 49-66), the two fallback constructors
   (lines 255, 359), the embedding probe destructuring (line 615), and every site that emits
   the string `"degraded"` (lines 558, 815, 824, 831, 946).
3. Execute in 3 micro-phases:
   - **Phase 1 — Types & Contracts**: add `degraded_reasons` to the health response. Add the
     doc comment on `EmbeddingCoverage` documenting the `total == 0` semantic. Fix `Default`.
   - **Phase 2 — Core Logic**: populate `degraded_reasons` at each of the 5 emission sites with
     the actual cause. Do not restructure unrelated code.
   - **Phase 3 — Tests & Delivery**: add the 3 named tests. Run the full verification block.
     Create the PR immediately.
4. **Blocker Recovery**: if the compiler or tests fail, search the web for the exact error
   signature immediately, fix it, and document the fix in the PR body. NEVER stop and wait for
   feedback. NEVER reduce scope by deleting an existing test.

## Existing Code Patterns (MUST follow)

- `src/health/mod.rs` — `CheckStatus` enum + `Check { name, status, message }` shape already
  exists (see the `checks` vec at line 1407 area). Reuse it for new check entries.
- Error handling: `thiserror` for typed errors in libs, `anyhow` for binaries (see `AGENTS.md` §8).
- Comments in English, minimal density (`AGENTS.md` §8).
- Golden rule: never call Rayon `.par_iter()` directly inside a Tokio worker — wrap in
  `tokio::task::spawn_blocking` (`AGENTS.md` §8). This issue should not need Rayon at all.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep -c "^error"` == 0
- [ ] `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep -c "^error"` == 0
- [ ] `cargo test --package xavier --lib --features ci-safe embedding_coverage -- --test-threads=1 2>&1 | grep "test result"` shows 0 failed
- [ ] `cargo test --package xavier --lib --features ci-safe degraded_reasons -- --test-threads=1 2>&1 | grep "test result"` shows 0 failed
- [ ] `cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` produces NO output (no regression in the pre-existing suite)
- [ ] `grep -n "percent: 100.0" src/health/mod.rs` returns NO match inside `impl Default for EmbeddingCoverage`
- [ ] `grep -c "degraded_reasons" src/health/mod.rs` >= 6 (field decl + 5 emission sites)
- [ ] `cargo fmt --all -- --check` produces no diff
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/health/mod.rs` | ~1140 lines, `EmbeddingCoverage` at 31, `Default` at 38, probe at 615, 5 `"degraded"` sites | Fix `Default`, add `degraded_reasons`, populate at 5 sites, add 3 tests | MED |

## DO NOT touch (Anti-Regression)

- `src/server/mcp/tools_core.rs` — **island of WAVE-29.02**. You will see it reading
  `health.embedding.connected`; that is expected and is being fixed in parallel. DO NOT edit it.
- `src/adapters/inbound/http/routes.rs`, `src/adapters/inbound/http/v1_api.rs` — they serialize
  the health struct; adding a field is enough, no edit needed there.
- `src/self_manage/mod.rs` — island of WAVE-29.03.
- `src/context/skill_dispatcher.rs`, `src/context/skill_registry.rs` — island of WAVE-29.04.
- `.gitcore/features.json` — reconciled by the orchestrator at wave end. NEVER hand-edit.
- `Cargo.toml` / `Cargo.lock` — no new dependencies. If you believe you need one, stop and
  explain in the PR body instead of adding it.
- The string values of `status` (`healthy`/`degraded`/`unhealthy`/`critical`) — consumers depend
  on them. Adding fields is allowed; changing these values is not.

## Anti-Hallucination Guard ⚠️

1. **READ before write**: read `src/health/mod.rs` fully before editing. The 5 `"degraded"`
   sites are in different functions with different local context.
2. **Do not invent fields**: `EmbeddingCoverage` has exactly 4 fields today. If you need a 5th,
   say so in the PR body and justify it.
3. **Do not "fix" the `connected` probe** — that is a separate defect owned by WAVE-29.02.
   Your job is to stop the *reporting* layer from lying about coverage and degradation causes.
4. **`cargo check` gate**: 0 errors before commit.
5. **No features.json edit.**
6. Verify the crate name is `xavier` (`cargo check --package xavier`) — not `xavier-core`.

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows `src/health/mod.rs` modified BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty and lists `src/health/mod.rs`
- [ ] The PR contains ≥1 real source file: `git show HEAD --name-only | grep -c "src/health/mod.rs"` >= 1
- [ ] `grep -c "#\[test\]" src/health/mod.rs` >= 3 new tests present (count before/after and show both in the PR body)
- [ ] PR body includes the **before** and **after** of `Default for EmbeddingCoverage`
- [ ] PR body includes the actual `cargo test` output tail proving 0 failures
- [ ] If the work could not be completed: DO NOT open a PR — comment the blocker on this issue.

## Verification

```bash
cargo fmt --all -- --check
cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l   # expect 0
cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | tail -20
```

## Dependencies & Merge Order

- **Depends on:** nothing. This is merge order **1/10**.
- **Parallel with:** 29.03, 29.04, 29.05, 29.06, 29.07, 29.08, 29.09, 29.10 (disjoint files).
- **Blocks:** nothing directly, but 29.10 (E2E) asserts against the new `degraded_reasons` field,
  so 29.10 must rebase onto this one.
- **Expected effort:** Medium 2-4h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix errors, do NOT commit broken code |
| Pre-existing tests fail | Verify they fail on `origin/main` too (`git stash && cargo test`) before assuming you caused it |
| `EmbeddingCoverage` has more fields than documented | Re-read the struct; do not assume. Report in PR body |
| Field name collides with an existing serde rename | Use `#[serde(default, skip_serializing_if = "...")]` and document why |
| Clippy `-D warnings` fails | Fix the lint or add a targeted `#[allow(...)]` WITH a comment justifying it |
