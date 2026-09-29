# [PAGEINDEX.08] MCP registration: tools_pageindex.rs (LAST in F1)

> Wave pageindex — F1 | Feature: `feat-pageindex-tree-core` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 06 (07 optional); REBASE on main AFTER V6/V6b/V6c secrets merge | Parallel group: G5 (alone, last) | Risk: MED (merge conflict zone) | Effort: Small 1-2h
> Worktree/branch: dedicated `feat/pageindex-08` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- Tools are defined per module (`get_xavier_context_tools()` etc.) and dispatched by name in `src/server/mcp/server.rs:105-118`. `mod.rs`/`server.rs`/`tools_core.rs` are being edited by the V6 secrets wave.

## Desired State (DELTA)

`tools_pageindex.rs` wraps `pageindex_glue::tools` into `MCPTool` defs (`get_pageindex_tools()`) and `handle_pageindex_tool(state, workspace, role, name, args)`. The ONLY edits to shared MCP files: `mod.rs` `pub mod tools_pageindex;` (+cfg), `server.rs` one line extending the tool list and one `else if name.starts_with("pageindex_")` dispatch arm. No role arm in `server.rs` (gating is inside the handler). MUST be done after rebasing on main once V6 has merged; if V6 is not merged, wait. Also live-verify after service restart (`tools/list` shows 5 tools).

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `src/server/mcp/tools_pageindex.rs` | new (all logic + tests here) | MED |
| `src/server/mcp/mod.rs` | +1 line | MED |
| `src/server/mcp/server.rs` | +2 small hunks | MED |

## Tests that prove it

- `test_mcp_pageindex_tools_announced`
- `test_mcp_pageindex_roundtrip_ingest_structure_pages`
- `test_mcp_existing_tool_names_unchanged`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier --lib --features ci-safe test_mcp_`
- [ ] `cargo clippy -p xavier --all-targets --features ci-safe -- -D warnings`
- [ ] `git diff origin/main --stat -- src/server/mcp # only the 3 files above`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- `src/server/mcp/tools_context.rs` (skill tool defs), `src/server/mcp/server.rs:1-30,97-118`, REQ-064
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- `tools_core.rs`; any existing tool name/schema
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
