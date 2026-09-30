# [WAVE-29.08] feat-codegraph-dump-path — `xavier code dump .` ignores its path argument and cwd

> Wave 29 — Codegraph self-introspection. Labels: `wave-29`, `ola29` (NO `jules` yet)
> Merge order: 8/10 | Risk: LOW | Effort: Small 1-2h

---

## Current State (MEDIBLE)

Files: `src/cli/handlers/code.rs` (`dump` / `scan` subcommands), `code-graph/` (the indexer crate)

**Defect 1 — `dump` ignores its path argument and the current working directory.**
Measured. The command was run from `/home/belal` with the argument `.`:
```
$ cd /home/belal
$ xavier code dump .
{ "status": "ok",
  "message": "Code graph dumped to /home/belal/proyectosSWAL/apps/xavier/.xavier/codegraph.json",
  "path": "/home/belal/proyectosSWAL/apps/xavier/.xavier/codegraph.json" }
```
The cwd was `/home/belal` and the argument was `.` (= `/home/belal`), yet it wrote to
`/home/belal/proyectosSWAL/apps/xavier/.xavier/`. The path argument is discarded and the
configured project root is used instead. A user asking to dump a different codebase silently gets
the default one, with a success message and a plausible-looking absolute path.

**Defect 2 — the codegraph does not cover Xavier itself, and the hub tools are useless on it.**
Measured via the MCP tools against the same database:
```
codegraph_explore("xavier") -> 10 symbols, but all from ANOTHER project:
    backend/src/application/services/xavier_client.rs
    backend/src/decision_engine/xavier_agent.rs
    backend/src/application/system/builder.rs
    (pump_detector.rs, paper_validation.rs, strategy_graduation.rs, ...)
get_code_graph -> total_symbols 14486, total_files 380, source "live_db"
codegraph_gods -> {"god_nodes": [], "returned": 0}
```
So the index contains 14486 symbols belonging to a *different* codebase, and the "god nodes"
tool returns nothing.

`get_code_graph` reported the 10 "hubs" and **all 10 were named `new`** — Rust constructors —
each with `incoming: 736, outgoing: 226/230`. The centrality ranking is rewarding generic
constructors. `codegraph_gods` filters those out as builtins, which is why it returns 0. Both
tools are therefore useless on a real corpus.

**Defect 3 — the portable dump is reported as absent even when it exists.**
`xavier_get_code_graph` returned `dump_present: false` with the hint
`Run 'xavier code dump .' or 'xavier code scan .'`, at a moment when a dump did exist on disk
(it had just been created). The presence check and the CLI disagree.

`code_graph.db` (163MB) was last modified 2026-09-20 while the service was actively running —
the index is 6 days stale, with no visible signal that it is stale.

Side effect of this audit, for transparency: the `dump .` command above created
`apps/xavier/.xavier/codegraph.json`. That file is a runtime cache; per `AGENTS.md` §10 it must
not be committed.

## Desired State (DELTA)

- **`dump` / `scan` must honour their path argument and the cwd.** Resolve the target from the
  argument; if the argument is absent, use the cwd. Only fall back to the configured project root
  when the user explicitly asks for it (or when no argument is possible). The `path` in the JSON
  response must be the path actually written.
- **Report the resolved target explicitly** in the response so a mismatch is visible, e.g. add
  `requested_path` and `resolved_path` fields.
- **Hub ranking must exclude generic constructors.** In the centrality computation, filter out
  trivial symbols: `new`, `default`, `from`, `clone`, `with_capacity`, `with_*`, and simple
  accessors. Deduplicate by name so 10 copies of `new` cannot occupy every slot. If the honest
  result is "no significant hubs", return that explicitly with a reason rather than a bare `[]`.
- **`dump_present` must be truthful.** Make the presence check agree with what `dump` writes.
  Prefer a single source of truth for the dump location.
- **Staleness must be visible.** `get_code_graph` (or `code stats`) should report the index
  timestamp and flag a stale index. Do NOT implement automatic reindexing in this issue — only
  make staleness observable.
- **New tests**:
  - `dump_respects_explicit_path_argument`
  - `dump_respects_cwd_when_no_argument`
  - `hubs_exclude_generic_constructors`
  - `dump_present_matches_actual_file`
- Risk: LOW. `src/cli/handlers/code.rs` already has tests (e.g. around lines 2550, 2622) — keep
  them passing.

## 🌐 Web Research Required

**MANDATORY — 4 queries. Research BEFORE implementing.**
1. search: "Rust resolve path argument vs cwd default precedence CLI"
2. search: "code graph centrality betweenness exclude trivial nodes constructors"
3. search: "index staleness detection report last modified timestamp"
4. search: "Rust dedupe by name HashMap retain order preserving"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read these files COMPLETELY before editing:
   - `src/cli/handlers/code.rs` — the `dump`, `scan`, `hubs`, and `stats` subcommand handlers,
     and the existing tests (around lines 2550, 2622)
   - `code-graph/src/` — the ranking/centrality code that produces `incoming`/`outgoing` counts
   - the code that computes `dump_present` (grep for it; it is surfaced by the MCP
     `get_code_graph` tool, whose definition is in `src/server/mcp/` — READ, DO NOT EDIT)
3. Write the failing test FIRST for the path-argument bug: assert `dump(some_tmp_dir)` writes
   under `some_tmp_dir`, not under the project root. Use a temp directory; do not write into the
   real repo during tests.
4. Execute in 3 micro-phases:
   - **Phase 1 — Types & Contracts**: resolved-vs-requested path in the response; staleness field.
   - **Phase 2 — Core Logic**: path resolution precedence; constructor filtering; truthful
     `dump_present`; staleness reporting.
   - **Phase 3 — Tests & Delivery**: 4 named tests, verification, PR.
5. **Blocker Recovery**: on any failure, search the web for the exact error signature, fix it,
   document it in the PR body. NEVER wait for feedback."

## Existing Code Patterns (MUST follow)

- `src/cli/handlers/code.rs` is a large handler module with an existing test module — add tests
  there, do not create a parallel test file.
- Use `std::path::Path` / `PathBuf` composition consistent with the rest of the file.
- Runtime caches belong under `.xavier/` and must never be committed (`AGENTS.md` §10). Make sure
  your tests do not leave artifacts in the repo.
- Comments in English, minimal density.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep -c "^error"` == 0
- [ ] `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep -c "^error"` == 0
- [ ] `cargo test --package xavier --lib --features ci-safe code -- --test-threads=1 2>&1 | grep "test result"` shows 0 failed
- [ ] `cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` produces NO output
- [ ] `cargo test --package code-graph 2>&1 | grep "test result: FAILED"` produces NO output
- [ ] PR body shows the BEFORE/AFTER of `dump` invoked from a directory that is NOT the project root
- [ ] `grep -n "requested_path\|resolved_path" src/cli/handlers/code.rs` >= 2 matches
- [ ] `grep -rn "index_age\|stale\|last_modified" src/cli/handlers/code.rs code-graph/src/` >= 1 match
- [ ] `cargo fmt --all -- --check` produces no diff
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/cli/handlers/code.rs` | `dump`/`scan`/`hubs` handlers; tests ~2550, 2622 | Path resolution precedence, requested/resolved reporting, constructor filtering, staleness, 4 tests | LOW |
| `code-graph/src/` (ranking module) | produces `incoming`/`outgoing`, rewards `new` | Exclude trivial constructors; dedupe by name | MED |

## DO NOT touch (Anti-Regression)

- `src/server/mcp/**` (`tools_core.rs`, `types.rs`, `tests.rs`, `tools_context.rs`) — islands of
  WAVE-29.02 / 29.10. `dump_present` is surfaced through MCP; read that code, do NOT edit it.
- `src/health/mod.rs` — island of WAVE-29.01.
- `src/self_manage/mod.rs` — island of WAVE-29.03.
- `src/context/**` — island of WAVE-29.04.
- `src/cli/handlers/doctor.rs` — island of WAVE-29.05.
- `src/cli/commands/improve.rs`, `src/auto_improvement/**` — island of WAVE-29.06.
- `src/cli/handlers/system.rs` — island of WAVE-29.07.
- `src/memory/**` — island of WAVE-29.09.
- `crates/xavier-core-logic/**`, `crates/xavier-wasm/**`, `vendor/maloca-core/**`.
- `.gitcore/features.json`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/**`.
- Do NOT run a full reindex of the 14486-symbol database in this issue, and do NOT commit
  `.xavier/codegraph.json` or any `*.db`.

## Anti-Hallucination Guard ⚠️

1. **READ before write.** `code.rs` is large (2600+ lines). Locate the actual `dump` and `hubs`
   handlers with grep before editing; do not assume line numbers.
2. **Do not change where dumps are stored by default** unless the current location is wrong —
   the bug is that the ARGUMENT is ignored, not that the default is misplaced. Changing the
   default would break existing callers.
3. **Do not implement automatic background reindexing.** Staleness must become VISIBLE, not
   silently fixed. Automatic reindexing is a separate, larger change.
4. **Do not delete the existing symbols from the index.** Filter at ranking time, not at index
   time, so the raw index stays complete.
5. **Do not write into the real repo during tests.** Use a temp directory.
6. **`cargo check` gate**: 0 errors before commit.

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows the modified files BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty
- [ ] `git show HEAD --name-only | grep -cE "src/cli/handlers/code.rs|code-graph/src"` >= 1
- [ ] `git status --porcelain` shows NO new `.xavier/codegraph.json` and NO `*.db` staged
- [ ] PR body shows the BEFORE/AFTER of `dump` run from `/tmp` with an explicit argument
- [ ] PR body lists exactly which symbol names are now excluded from hub ranking and why
- [ ] If the work could not be completed: DO NOT open a PR — comment the blocker on this issue.

## Verification

```bash
cargo fmt --all -- --check
cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l   # expect 0
cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | tail -20
cargo test --package code-graph 2>&1 | tail -10
```

## Dependencies & Merge Order

- **Depends on:** nothing. Merge order **8/10**.
- **Parallel with:** 29.01, 29.02, 29.03, 29.04, 29.05, 29.06, 29.07, 29.09, 29.10.
- **Expected effort:** Small 1-2h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix errors, do NOT commit broken code |
| `code-graph` is a separate crate with its own MSRV/lints | Match its existing lint level; do not tighten it repo-wide |
| Filtering constructors empties the hub list entirely | Then the corpus genuinely has no hubs — return an explicit empty result WITH a reason string, and say so in the PR body |
| Existing `code.rs` tests assert the old path behaviour | They encode the bug — update them and justify it explicitly |
| `dump_present` lives in a file you believe you must edit | It is exposed via `src/server/mcp/` (island of 29.02/29.10). Do NOT edit it — make the CLI side truthful and document the MCP discrepancy in the PR body |
