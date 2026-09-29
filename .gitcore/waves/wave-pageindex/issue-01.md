# [PAGEINDEX.01] Scaffold crate xavier-pageindex + tree data model

> Wave pageindex — F1 | Feature: `feat-pageindex-tree-core` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: none | Parallel group: G0 (first, alone) | Risk: LOW | Effort: Small 1-2h
> Worktree/branch: dedicated `feat/pageindex-01` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- `crates/xavier-pageindex/` does not exist. Root `Cargo.toml` `[workspace] members` lists 10 crates (no pageindex).

## Desired State (DELTA)

New workspace member. ALSO creates EMPTY stub files (module doc comment only) for every module later issues fill, and declares them all in `lib.rs`, so later issues never edit `lib.rs`/`mod.rs` declarations: `builders/{mod,markdown,plain,legal}.rs`, `store/{mod,schema,sqlite}.rs`, `query.rs`, `optimize.rs`, `summarize.rs`, `eval/{mod,metrics}.rs`, and cfg-gated `pdf/{mod,outline,layout,pdfium_loader,cascade}.rs`. Real content: `error.rs`, `model.rs` (Document, TreeNode, DocumentTree, PageUnit, DocStatus, SourceKind, `DocumentTree::validate`, preorder zero-padded `node_id` assignment) and an empty `PageIndex` facade stub. Crate must NOT depend on `xavier`. Default features: `sqlite`, `markdown` declared (deps may be declared but unused until later issues).

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `Cargo.toml (root)` | add `"crates/xavier-pageindex"` to workspace members ONLY | LOW |
| `crates/xavier-pageindex/Cargo.toml` | new: declares ALL deps and features up front (serde, serde_json, thiserror, sha2, optional rusqlite, pulldown-cmark, lopdf, pdfium-render without `static`) so no later issue edits this file or Cargo.lock | LOW |
| `crates/xavier-pageindex/src/lib.rs` | new | LOW |
| `crates/xavier-pageindex/src/error.rs` | new | LOW |
| `crates/xavier-pageindex/src/model.rs` | new | LOW |
| `crates/xavier-pageindex/src/{builders,store,pdf,eval}/**, query.rs, optimize.rs, summarize.rs` | new: empty module stubs (see Desired State) | LOW |

## Tests that prove it

- `test_tree_node_serde_roundtrip_stable_json`
- `test_node_ids_are_zero_padded_preorder`
- `test_tree_validate_rejects_overlapping_siblings`
- `test_tree_validate_rejects_child_outside_parent_range`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier-pageindex`
- [ ] `cargo clippy -p xavier-pageindex --all-targets -- -D warnings`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo tree -p xavier-pageindex | grep -c '^xavier ' # must print 0`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- `src/documents/legal_chunker.rs` (LegalChunk shape), spec sections 2-3
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- Any file under `src/`; `crates/xavier-core-logic`; other root Cargo.toml sections
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
