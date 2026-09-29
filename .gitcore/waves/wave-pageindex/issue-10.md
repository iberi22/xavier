# [PAGEINDEX.10] PDF layout heading detector (pdfium, runtime-bound)

> Wave pageindex — F2 | Feature: `feat-pageindex-pdf` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 01 | Parallel group: G6 parallel {09,10,11} | Risk: HIGH (native lib, heuristics) | Effort: Large 4-6h
> Worktree/branch: dedicated `feat/pageindex-10` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- No font/position data anywhere in the repo. nixpkgs provides `pdfium-binaries` (7087).

## Desired State (DELTA)

Feature `pdf-layout` (`pdfium-render`, dynamic binding ONLY, `static` feature forbidden): `pdf::layout::detect_headings(bytes) -> Option<Vec<HeadingCandidate{title,page,level}>>`. Heuristics: char-level font size/weight per line, body-size = mode, size clusters above body map to levels 1..N, bold short lines at body size = lowest level, repeated top/bottom lines across pages filtered as headers/footers. Library lookup order per spec sec. 4 (`XAVIER_PAGEINDEX_PDFIUM_LIB` -> `PDFIUM_DYNAMIC_LIB_PATH` -> next to binary -> system); missing lib => `PdfiumUnavailable`, never a panic. Pure heuristic logic is separated from pdfium calls so clustering is unit-testable with synthetic line records; pdfium-backed tests are `#[ignore]` unless the lib resolves. Document NixOS (`shell.nix` note) and the optional non-required CI job in this issue's PR description; `shell.nix`/workflows edited only if the owner approves.

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `crates/xavier-pageindex/src/pdf/layout.rs` | fill stub from issue 01 | HIGH |
| `crates/xavier-pageindex/src/pdf/pdfium_loader.rs` | fill stub from issue 01 | HIGH |
| `crates/xavier-pageindex/tests/pdf_layout.rs` | new | HIGH |

## Tests that prove it

- `test_layout_font_size_clusters_map_to_levels`
- `test_layout_headers_footers_filtered`
- `test_layout_bold_short_line_is_heading_candidate`
- `test_layout_missing_pdfium_returns_typed_error_not_panic`
- `test_layout_pdfium_path_resolution_env_order`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier-pageindex --features pdf-layout`
- [ ] `cargo check -p xavier-pageindex # default, no pdfium needed`
- [ ] `PDFIUM_DYNAMIC_LIB_PATH=$(nix eval --raw nixpkgs#pdfium-binaries.outPath)/lib cargo test -p xavier-pageindex --features pdf-layout -- --include-ignored`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- PageIndex `pageindex/flash/heading_detection/`, `parser_pdfium_charlevel/` (idea only); pdfium-render docs
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- `src/`; `shell.nix`/`.github/**` unless owner approves
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
