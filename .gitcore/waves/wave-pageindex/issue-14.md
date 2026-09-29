# [PAGEINDEX.14] Eval harness + DocBot vs tree vs hybrid comparison

> Wave pageindex — F3 | Feature: `feat-pageindex-hybrid` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 05 (harness), 13 (hybrid column) | Parallel group: G8 parallel {13,14} | Risk: MED | Effort: Medium 4h
> Worktree/branch: dedicated `feat/pageindex-14` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- `src/retrieval/eval.rs` scores text hits; no page-range metric; no fixtures for structured docs.

## Desired State (DELTA)

Crate `eval` module (`HitAtK`, `Mrr`, `page_range_overlap`), fixtures `tests/fixtures/pageindex/` (>= 3 documents: markdown manual, legal contract, longer text; >= 30 queries with gold `{doc,start_page,end_page}`), deterministic lexical tree navigator baseline (BM25 over titles+summaries, greedy descent), optional loader for PageIndex-OSS-Benchmark JSONL via `PAGEINDEX_OSS_BENCHMARK_DIR` (skipped if unset; never downloaded in CI). `tests/pageindex_eval.rs` compares DocBot retrieval, tree navigator and hybrid, prints hit@1/hit@3/MRR table; final step: record measured numbers in the spec sec. 10 and update ADR-036 verification note. Hybrid column asserts only after issue 13 merged.

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `crates/xavier-pageindex/src/eval/mod.rs` | fill stub | MED |
| `crates/xavier-pageindex/src/eval/metrics.rs` | fill stub | MED |
| `tests/pageindex_eval.rs` | new | MED |
| `tests/fixtures/pageindex/**` | new | MED |
| `docs/features/specs/FEATURE-pageindex-tree-retrieval.md` | append measured results | MED |
| `docs/adr/ADR-036-tree-reasoning-retrieval.md` | update verification note | MED |

## Tests that prove it

- `test_eval_page_range_hit_metric`
- `test_eval_hit_at_k_and_mrr`
- `test_eval_loads_benchmark_jsonl`
- `test_eval_fixture_set_has_gold_ranges`
- `test_eval_tree_navigator_lexical_baseline_deterministic`
- `test_eval_report_compares_docbot_tree_hybrid`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier-pageindex eval_`
- [ ] `cargo test -p xavier --features ci-safe --test pageindex_eval -- --nocapture`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- `src/retrieval/eval.rs`, `src/rag/pipeline.rs`, PageIndex-OSS-Benchmark README
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- `src/retrieval/gating.rs` (issue 13 owns it)
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
