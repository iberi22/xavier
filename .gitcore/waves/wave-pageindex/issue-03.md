# [PAGEINDEX.03] Plain-text and legal tree builders

> Wave pageindex — F1 | Feature: `feat-pageindex-tree-core` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 01 | Parallel group: G1 parallel {02,03,04} | Risk: MED | Effort: Medium 3h
> Worktree/branch: dedicated `feat/pageindex-03` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- `src/documents/legal_chunker.rs` has the Capitulo/Articulo/Clausula/Parrafo/Preambulo regex table (lines 67-99) but emits flat chunks with `full_path`.

## Desired State (DELTA)

`builders::plain` (numbered headings `1.`, `1.1`, `Chapter N`, ALL-CAPS short lines; fallback to fixed windows when none found) and `builders::legal` (regex table PORTED, Spanish+English; chapters contain articles contain clauses/paragraphs). Xavier-side parity test compares node titles/order with `LegalHierarchicalChunker::chunk_document` on a shared fixture (lives in issue 06 glue tests to keep the crate independent; this issue ships the fixture files + a crate-side golden JSON).

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `crates/xavier-pageindex/src/builders/plain.rs` | fill stub from issue 01 | MED |
| `crates/xavier-pageindex/src/builders/legal.rs` | fill stub from issue 01 | MED |
| `crates/xavier-pageindex/tests/fixtures/legal/*` | new fixtures + golden json | MED |
| `crates/xavier-pageindex/tests/plain_legal_builders.rs` | new | MED |

## Tests that prove it

- `test_plain_numbered_headings_detected`
- `test_plain_no_headings_falls_back_to_fixed_windows`
- `test_legal_articles_nest_under_chapters`
- `test_legal_spanish_and_english_headings`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier-pageindex plain_ legal_`
- [ ] `cargo clippy -p xavier-pageindex --all-targets -- -D warnings`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- `src/documents/legal_chunker.rs` lines 60-320
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- `src/documents/legal_chunker.rs` (read-only reference)
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
