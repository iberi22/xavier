# [WAVE-29.07] feat-cli-health-stats-parity — CLI reports `Version: unknown` and empty stats vs MCP

> Wave 29 — CLI reliability + parity. Labels: `wave-29`, `ola29` (NO `jules` yet)
> Merge order: 7/10 | Risk: LOW | Effort: Small 1-2h

---

## Current State (MEDIBLE)

File: `src/cli/handlers/system.rs`

**Defect 1 — `xavier health` cannot report its own version.**
Measured:
```
$ xavier health
═══════════════════════════════════════════
  System Health Status
═══════════════════════════════════════════
  Status:      degraded
  Version:     unknown
═══════════════════════════════════════════
```
While, in the same binary:
```
$ xavier --version   -> xavier 0.2.15
$ xavier stats       -> { "status": "ok", "version": "0.2.15", "workspace_id": "default" }
```
So the version is available in two other code paths but `health` renders `unknown`. An operator
cannot tell what build they are looking at from the health output.

Also note the same `degraded` ambiguity documented in WAVE-29.01: the health command prints a bare
`degraded` with no indication of cause. In this instance every subsystem was in fact healthy and
the degradation came from host memory pressure.

**Defect 2 — `xavier stats` (CLI) and the `xavier_stats` MCP tool are different implementations.**
Measured side by side on the same process:
```
CLI  : xavier stats  -> {"status":"ok","version":"0.2.15","workspace_id":"default"}
MCP  : xavier_stats  -> {"total_memories":1482,"total_entities":20,
                        "semantic_entities":0,"semantic_relations":0,
                        "storage_bytes":10956148}
```
The CLI omits every substantive number. The MCP tool also reports
`total_entities: 20` against `total_memories: 1482` with `semantic_entities: 0` and
`semantic_relations: 0` — i.e. the semantic/graph layer is effectively empty, and nothing in
either output says so.

## Desired State (DELTA)

- **`xavier health`**:
  - Report the real version. Reuse the same source `xavier --version` / `xavier stats` use
    (`env!("CARGO_PKG_VERSION")` or the existing helper) — do not add a new mechanism.
  - When the overall status is `degraded`, print **why**. `src/health/mod.rs` is gaining a
    `degraded_reasons` field in WAVE-29.01; consume it **defensively**: if the field exists, use
    it; if you are merged before 29.01, degrade gracefully rather than failing to compile.
    Coordinate by reading the field through an accessor/serde path that tolerates absence.
  - Do not change the box-drawing layout style; extend it.
- **`xavier stats`**:
  - Return the SAME substantive numbers as the MCP `xavier_stats` tool: memory count, entity
    count, semantic entity/relation counts, storage bytes. Prefer ONE shared implementation with
    two thin wrappers over two divergent code paths — that is the actual defect.
  - Add the derived signal that is currently missing: surface the semantic-layer state
    (`semantic_entities: 0` against `total_entities: 20`) so an operator can see the graph layer
    is not populated. Do not attempt to FIX that here; only report it.
- **Both commands**: non-zero exit code when the reported status is unhealthy, so they are usable
  in scripts and CI. Document the mapping.
- **New tests**:
  - `health_command_reports_known_version`
  - `stats_cli_matches_mcp_stats_fields`
  - `stats_reports_semantic_layer_state`
- Risk: LOW. One caveat: if the cleanest fix is a shared helper in a new module, that is fine, but
  do not refactor `src/health/mod.rs` (island of WAVE-29.01) or `src/server/mcp/tools_core.rs`
  (island of WAVE-29.02).

## 🌐 Web Research Required

**MANDATORY — 4 queries. Research BEFORE implementing.**
1. search: "Rust CARGO_PKG_VERSION env macro vs runtime version lookup"
2. search: "single source of truth shared implementation two CLI wrappers"
3. search: "CLI exit codes health check automation scriptable"
4. search: "backward compatible consume new optional field serde default Rust"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read these files COMPLETELY before editing:
   - `src/cli/handlers/system.rs` (all of it) — find the `health` and `stats` handlers
   - the CLI command wiring for `health` and `stats` (grep the command enum) to see how the
     handlers are reached and where the exit code is decided
   - `src/server/mcp/tools_core.rs` around the `xavier_stats` arm to see EXACTLY which fields
     the MCP tool produces. READ IT, DO NOT EDIT IT — it is WAVE-29.02's island.
   - how `xavier --version` resolves the version, so you reuse that mechanism
3. Execute in 3 micro-phases:
   - **Phase 1 — Types & Contracts**: identify the shared stats shape; decide how to consume
     `degraded_reasons` without a hard compile dependency on WAVE-29.01.
   - **Phase 2 — Core Logic**: real version in `health`; parity in `stats`; semantic-layer
     reporting; exit codes.
   - **Phase 3 — Tests & Delivery**: 3 named tests, verification, PR.
4. **Blocker Recovery**: on any failure, search the web for the exact error signature, fix it,
   document it in the PR body. NEVER wait for feedback."

## Existing Code Patterns (MUST follow)

- The box-drawing report layout in `system.rs` is the established presentation; extend it, do
  not restyle it.
- The MCP `xavier_stats` payload is the reference contract for field names — match it exactly
  (snake_case as emitted: `total_memories`, `total_entities`, `semantic_entities`,
  `semantic_relations`, `storage_bytes`).
- Errors: `anyhow` in binaries (`AGENTS.md` §8).
- Comments in English, minimal density.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep -c "^error"` == 0
- [ ] `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep -c "^error"` == 0
- [ ] `cargo test --package xavier --lib --features ci-safe stats -- --test-threads=1 2>&1 | grep "test result"` shows 0 failed
- [ ] `cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` produces NO output
- [ ] `grep -rn "unknown" src/cli/handlers/system.rs | grep -i version` returns NO match
- [ ] `grep -c "total_memories\|total_entities\|storage_bytes" src/cli/handlers/system.rs` >= 3
- [ ] `git diff --stat HEAD -- src/server/mcp/tools_core.rs src/health/mod.rs` is EMPTY
- [ ] `cargo fmt --all -- --check` produces no diff
- [ ] PR body shows the BEFORE and AFTER of `xavier health` and `xavier stats` output
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/cli/handlers/system.rs` | `health` prints `Version: unknown`; `stats` returns 3 fields | Real version, degraded reasons, full stats parity, semantic-layer reporting, exit codes, 3 tests | LOW |
| NEW shared helper (your choice of path) | — | Single stats implementation used by both CLI and, if safe, referenced by MCP | LOW |

## DO NOT touch (Anti-Regression)

- `src/server/mcp/tools_core.rs` — **island of WAVE-29.02**. It contains the MCP `xavier_stats`
  arm you are mirroring. Read it, match its field names, do NOT edit it. If you believe the MCP
  side must change to share code, describe it in the PR body instead.
- `src/health/mod.rs` — **island of WAVE-29.01**, which is adding `degraded_reasons` right now.
  Do NOT add that field yourself. Consume it defensively.
- `src/self_manage/mod.rs` — island of WAVE-29.03.
- `src/context/**` — island of WAVE-29.04.
- `src/cli/handlers/doctor.rs` — island of WAVE-29.05.
- `src/cli/commands/improve.rs`, `src/auto_improvement/**` — island of WAVE-29.06.
- `src/cli/handlers/code.rs`, `code-graph/**` — island of WAVE-29.08.
- `src/memory/**` — island of WAVE-29.09.
- `.gitcore/features.json`, `Cargo.toml`, `Cargo.lock`, `tests/**`, `.github/workflows/**`.
- Do NOT rename the `health` or `stats` commands or change their flags.

## Anti-Hallucination Guard ⚠️

1. **READ before write.** Confirm the real field names of the MCP stats payload by reading
   `tools_core.rs`; do not assume them from this issue text.
2. **Do not hardcode `0.2.15`.** Use the version mechanism the binary already uses for
   `--version`. A hardcoded literal will silently rot on the next release.
3. **Do not create a compile-time dependency on `degraded_reasons`.** WAVE-29.01 lands in
   parallel; if you hard-require the field, one of the two PRs will fail to build depending on
   merge order.
4. **Do not attempt to populate the semantic layer.** It is empty (`semantic_entities: 0`).
   REPORT it; fixing extraction is not in scope for this issue.
5. **`cargo check` gate**: 0 errors before commit.

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows the modified files BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty
- [ ] `git show HEAD --name-only | grep -cE "src/cli/|src/stats"` >= 1
- [ ] PR body contains literal BEFORE/AFTER terminal output for BOTH commands
- [ ] PR body states how you avoided a hard dependency on WAVE-29.01's new field
- [ ] PR body lists the exit-code mapping
- [ ] If the work could not be completed: DO NOT open a PR — comment the blocker on this issue.

## Verification

```bash
cargo fmt --all -- --check
cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l   # expect 0
cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | tail -20
```

## Dependencies & Merge Order

- **Depends on:** nothing hard. Merge order **7/10**.
- **Soft dependency on:** 29.01 (adds `degraded_reasons`) and 29.02 (MCP stats arm). Both are
  in the SAME wave. If you need to rebase after they land, do so — do not edit their files.
- **Parallel with:** 29.01, 29.02, 29.03, 29.04, 29.05, 29.06, 29.08, 29.09, 29.10.
- **Expected effort:** Small 1-2h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix errors, do NOT commit broken code |
| Sharing code with MCP would require editing `tools_core.rs` | Do not. Keep the CLI-side helper and document the remaining duplication in the PR body |
| `degraded_reasons` is not yet available at your merge base | Use a defensive/serde-tolerant read; that is the expected case |
| An existing test asserts `Version: unknown` | That test encodes the bug — update it and say so explicitly |
| Exit-code change breaks a script | Find it with grep, update it, justify in the PR body |
