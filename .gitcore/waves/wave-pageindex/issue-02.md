# [PAGEINDEX.02] Markdown tree builder

> Wave pageindex — F1 | Feature: `feat-pageindex-tree-core` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 01 | Parallel group: G1 parallel {02,03,04} | Risk: LOW | Effort: Small 2h
> Worktree/branch: dedicated `feat/pageindex-02` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- `crates/xavier-pageindex/src/builders/` absent. `pulldown-cmark` 0.13 is already in the root manifest (reference only).

## Desired State (DELTA)

`builders::markdown::build(text, lines_per_page) -> (DocumentTree, Vec<Page>)`: ATX/setext headings to nested nodes via a level stack, headings inside fenced code ignored, skipped levels attach to nearest ancestor, preamble before first heading becomes a root node, virtual pages = fixed line windows, node ranges cover every page. Port of the idea of `page_index_md.py`, not its code.

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `crates/xavier-pageindex/src/builders/markdown.rs` | fill stub from issue 01 (do not edit builders/mod.rs) | LOW |
| `crates/xavier-pageindex/tests/fixtures/md/*.md` | new fixtures | LOW |
| `crates/xavier-pageindex/tests/markdown_builder.rs` | new | LOW |

## Tests that prove it

- `test_md_headings_build_nested_tree`
- `test_md_ignores_hashes_inside_code_fences`
- `test_md_skipped_heading_levels_attach_to_nearest_ancestor`
- `test_md_node_ranges_cover_all_pages`
- `test_md_preamble_becomes_root_node`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier-pageindex md_`
- [ ] `cargo clippy -p xavier-pageindex --all-targets -- -D warnings`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- `crates/xavier-pageindex/src/model.rs` (issue 01); PageIndex `pageindex/page_index_md.py` (idea only)
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- model.rs signatures (request changes via issue comment); `src/`
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
