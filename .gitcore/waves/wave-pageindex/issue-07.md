# [PAGEINDEX.07] HTTP routes /v1/pageindex/*

> Wave pageindex — F1 | Feature: `feat-pageindex-tree-core` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 06 | Parallel group: G4 parallel {07 with F2 issues 09,10} | Risk: LOW | Effort: Small 2h
> Worktree/branch: dedicated `feat/pageindex-07` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- `src/server/docbot_routes.rs` builds a router merged in `src/cli/server.rs` (~line 818).

## Desired State (DELTA)

`pageindex_routes::router(state)` implementing the six routes of spec sec. 7 over `pageindex_glue`, mounted under the same auth as DocBot. Responses reuse the tool envelopes. No MCP files touched.

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `src/server/pageindex_routes.rs` | new | LOW |
| `src/server/mod.rs` | add ONE line `pub mod pageindex_routes;` (cfg feature) | LOW |
| `src/cli/server.rs` | mount router next to docbot (few lines) | LOW |

## Tests that prove it

- `test_pageindex_routes_browse_ok`
- `test_pageindex_routes_structure_404_unknown_doc`
- `test_pageindex_routes_require_auth`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier --lib --features ci-safe pageindex_routes`
- [ ] `cargo clippy -p xavier --all-targets --features ci-safe -- -D warnings`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- `src/server/docbot_routes.rs`, `src/cli/server.rs` lines 780-830
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- `src/server/mcp/**`
- `.gitcore/features.json` — status promoted only by a green `scripts/verify-pipeline.sh`, never by hand.
- `src/server/mcp/{mod,server,tools_core}.rs` unless this is issue 08 (V6 secrets wave owns them).
- `src/context/skill_controller/*`, `src/context/mod.rs` (another wave).
- Any file assigned to a parallel issue of this wave (keep file islands disjoint).

## Anti-Hallucination Guard

1. READ before write; verify every referenced symbol with grep.
2. No invented crates: new deps only as listed in the spec (sec. 4); keep default/`ci-safe` free of native libs.
3. New `std::env::var` => add key to `.env.example` in the same PR (read only in `pageindex_glue/settings.rs`).
4. Note: main CI has a pre-existing intermittent failure in "Rust Integration (Observability Contract)" (#2701); do not chase it here.

## PR Delivery Requirements

- [ ] 1 PR = this issue only; conventional commit `feat(pageindex): ...`; no co-author/AI trailers.
- [ ] `git diff --stat origin/main` is non-empty and matches the files table.
- [ ] If blocked, do not open an empty PR; comment the blocker on the issue.
