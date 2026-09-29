# [PAGEINDEX.12] PDF cascade pipeline + ingest wiring (LLM ToC fallback)

> Wave pageindex — F2 | Feature: `feat-pageindex-pdf` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 09, 10, 11 | Parallel group: G7 (alone) | Risk: MED | Effort: Medium 3-4h
> Worktree/branch: dedicated `feat/pageindex-12` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- Facade returns `FeatureDisabled("pdf")` for `Source::Pdf`.

## Desired State (DELTA)

`pdf::cascade::build_pdf_tree`: bookmarks (`pdf-outline`) -> layout headings (`pdf-layout`, if pdfium loads) -> LLM ToC from first pages when a Summarizer/LLM is configured (idea of `page_index_classic.py`, ToC-page detection + verification of titles against page text) -> fixed windows. `Document.builder` records the strategy used. Without `pdf-outline` feature the facade returns `FeatureDisabled("pdf")`. Glue `pageindex-pdf`/`pageindex-pdfium` features pass through; `pageindex_index_document` accepts `format=pdf` (bytes via `path`).

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `crates/xavier-pageindex/src/pdf/cascade.rs` | fill stub from issue 01 | MED |
| `crates/xavier-pageindex/src/lib.rs` | modify: Source::Pdf dispatch | MED |
| `src/pageindex_glue/tools.rs` | modify: allow pdf format | MED |
| `crates/xavier-pageindex/tests/pdf_cascade.rs` | new | MED |

## Tests that prove it

- `test_pdf_cascade_prefers_bookmarks`
- `test_pdf_cascade_falls_back_to_layout`
- `test_pdf_cascade_falls_back_to_llm_toc_when_no_layout`
- `test_pdf_cascade_final_fallback_fixed_windows`
- `test_pdf_ingest_default_features_returns_feature_disabled_error`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier-pageindex --features pdf-outline`
- [ ] `cargo test -p xavier-pageindex # default features`
- [ ] `cargo test -p xavier --lib --features ci-safe pageindex_glue`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- issues 09-11 outputs; PageIndex `page_index_classic.py`
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
