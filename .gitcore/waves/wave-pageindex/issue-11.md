# [PAGEINDEX.11] Tree optimize + optional LLM summaries + summarizer adapter

> Wave pageindex — F2 | Feature: `feat-pageindex-pdf` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 05, 06 | Parallel group: G6 parallel {09,10,11} | Risk: MED | Effort: Medium 3-4h
> Worktree/branch: dedicated `feat/pageindex-11` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- `src/rag/llm_adapter.rs` offers `LlmAdapterTrait::complete` and `LlmConfig::from_env` (DOCBOT_LLM_*); `RetrievalOnly` returns chunks, not summaries.

## Desired State (DELTA)

Crate: `optimize::{merge_tiny, split_huge, optimize}` (thresholds `min/max_node_tokens`; splits on page boundaries; preserves page coverage; idempotent) and `summarize::{Summarizer, summarize_tree}` (bottom-up: leaf from its pages, parents from child summaries; bounded concurrency; cache keyed by sha256(title+text+model) so re-ingest is free; a failing node keeps `summary=None` and the tree stays valid). Glue: `pageindex_glue/summarizer.rs` implements `Summarizer` over `LlmAdapterTrait`; `RetrievalOnly` backend or `XAVIER_PAGEINDEX_SUMMARIZE=false` => no LLM call ever. Wire `IngestOptions.optimize/summarize` in the facade.

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `crates/xavier-pageindex/src/optimize.rs` | fill stub | MED |
| `crates/xavier-pageindex/src/summarize.rs` | fill stub | MED |
| `crates/xavier-pageindex/src/lib.rs` | modify facade ingest options body only | MED |
| `src/pageindex_glue/summarizer.rs` | new | MED |
| `src/pageindex_glue/mod.rs` | +1 line `mod summarizer;` | MED |
| `crates/xavier-pageindex/tests/optimize_summarize.rs` | new | MED |

## Tests that prove it

- `test_optimize_merges_tiny_leaf_into_prev_sibling`
- `test_optimize_splits_node_over_token_limit_on_page_boundaries`
- `test_optimize_preserves_page_coverage`
- `test_optimize_is_idempotent`
- `test_summarize_bottom_up_order`
- `test_summarize_cache_hit_skips_llm`
- `test_summarize_failure_leaves_summary_none_and_tree_valid`
- `test_summarizer_adapter_uses_llm_config`
- `test_tree_builds_with_no_llm_configured`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier-pageindex -- optimize_ summarize_ test_tree_builds`
- [ ] `cargo test -p xavier --lib --features ci-safe test_summarizer_adapter`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- PageIndex `pageindex/tree_optimize.py` (idea), `src/rag/llm_adapter.rs`
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- `src/rag/llm_adapter.rs` (read-only; no new env vars beyond spec sec. 8)
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
