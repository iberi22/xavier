# [PAGEINDEX.05] PageIndex facade + query operations

> Wave pageindex — F1 | Feature: `feat-pageindex-tree-core` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 02, 03, 04 | Parallel group: G2 (alone) | Risk: MED | Effort: Medium 3h
> Worktree/branch: dedicated `feat/pageindex-05` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- Builders and store exist independently; no orchestration.

## Desired State (DELTA)

`PageIndex::{ingest, browse, get_document, get_structure, get_pages, delete}` (spec sec. 5). `query.rs`: structure projection without page text (`max_depth`, `node_id` subtree), char-budget truncation with `truncated:true` marker, page-range parser (`"5-7,12"`, max 20 pages, 1-based, validates against page_count), similar-name hints (top 3 by edit-distance ratio) for unknown docs, `structure_first` flag when `page_count > 20`. Source dispatch: Markdown/PlainText/Legal now; `Pdf` returns `FeatureDisabled("pdf")` until F2.

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `crates/xavier-pageindex/src/lib.rs` | modify: facade impl body (declarations already present) | MED |
| `crates/xavier-pageindex/src/query.rs` | fill stub | MED |
| `crates/xavier-pageindex/tests/facade.rs` | new | MED |

## Tests that prove it

- `test_browse_lists_docs_with_status_and_page_count`
- `test_get_document_structure_strips_text`
- `test_structure_truncates_to_char_budget_with_marker`
- `test_get_page_content_parses_ranges`
- `test_get_page_content_rejects_out_of_range`
- `test_get_page_content_respects_response_char_cap`
- `test_unknown_doc_returns_similar_names`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier-pageindex`
- [ ] `cargo clippy -p xavier-pageindex --all-targets -- -D warnings`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- spec sections 5-6; PageIndex `pageindex/agent_tools.py` (response caps, error envelopes, similar names)
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- `src/`
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
