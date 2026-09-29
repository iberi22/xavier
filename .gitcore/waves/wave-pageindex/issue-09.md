# [PAGEINDEX.09] PDF outline builder (bookmarks, pure Rust)

> Wave pageindex — F2 | Feature: `feat-pageindex-pdf` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 01 | Parallel group: G6 parallel {09,10,11} | Risk: MED | Effort: Medium 3h
> Worktree/branch: dedicated `feat/pageindex-09` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- `src/documents/pdf.rs` is a naive BT/ET extractor without outlines. `lopdf` is not in `Cargo.lock`.

## Desired State (DELTA)

Feature `pdf-outline` (`lopdf`): `pdf::outline::{extract_outline, extract_pages}` returning bookmark tree (title, dest page; resolves named destinations and indirect refs) and per-page text; `None` when no outline; typed `Encrypted` error; malformed PDFs never panic. Tiny fixture PDFs generated in-test with lopdf (no binary blobs > 20 KB).

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `crates/xavier-pageindex/src/pdf/outline.rs` | fill stub from issue 01 | MED |
| `crates/xavier-pageindex/tests/pdf_outline.rs` | new | MED |

## Tests that prove it

- `test_pdf_outline_bookmarks_to_tree`
- `test_pdf_outline_resolves_named_destinations`
- `test_pdf_without_outline_returns_none`
- `test_pdf_page_text_extracted_per_page`
- `test_pdf_encrypted_returns_typed_error`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier-pageindex --features pdf-outline`
- [ ] `cargo check -p xavier-pageindex (default: no lopdf compiled)`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- PageIndex `pageindex/flash/embedded_toc.py` (idea)
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- `src/documents/pdf.rs`; `src/`; root Cargo.toml
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
