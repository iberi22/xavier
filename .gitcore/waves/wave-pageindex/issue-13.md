# [PAGEINDEX.13] Hybrid retrieval arm + gating hook

> Wave pageindex — F3 | Feature: `feat-pageindex-hybrid` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Depends on: 06, 11 | Parallel group: G8 parallel {13,14} | Risk: HIGH (gating.rs is 1770 lines, widely used) | Effort: Large 4-5h
> Worktree/branch: dedicated `feat/pageindex-13` from `feat/pageindex` (or main once merged). One agent, one session.

---

## Current State (MEDIBLE)

- `AdaptiveGating` fuses layers with `reciprocal_rank_fusion(rrf_k)`; config in `GatingConfig`.

## Desired State (DELTA)

`retrieval/pageindex_arm.rs`: (a) `embed_document(doc)` writes each node summary (fallback: title + first 300 chars) into the Xavier store via the existing memory API with tags `pageindex`, `doc:<name>`, `node:<id>` and metadata `{start_page,end_page}`, replacing stale nodes on re-ingest; (b) `search(query, ws, k) -> Vec<ScoredResult>` = BM25+vector candidates restricted to `pageindex` tag, each carrying doc/node/page range. `gating.rs`: minimal hook adding the arm's ranked list to the RRF input ONLY when `XAVIER_PAGEINDEX_ARM_ENABLED=true`, weight `XAVIER_PAGEINDEX_ARM_WEIGHT`; errors are logged and ignored (fail-open); disabled path byte-identical. `retrieval/mod.rs` +1 line.

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `src/retrieval/pageindex_arm.rs` | new | HIGH |
| `src/retrieval/gating.rs` | minimal hook (<40 lines) | HIGH |
| `src/retrieval/mod.rs` | +1 line | HIGH |
| `src/pageindex_glue/settings.rs` | +2 fields (arm enabled/weight) | HIGH |
| `.env.example` | +2 vars | HIGH |

## Tests that prove it

- `test_arm_embeds_node_summaries_with_doc_and_node_tags`
- `test_arm_reingest_replaces_stale_embeddings`
- `test_arm_results_carry_doc_node_page_range`
- `test_arm_workspace_scoped`
- `test_gating_arm_disabled_by_default_no_behavior_change`
- `test_gating_rrf_fuses_arm_with_layers`
- `test_gating_arm_failure_is_fail_open`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier --lib --features ci-safe retrieval::`
- [ ] `cargo test -p xavier --lib --features ci-safe test_arm_ test_gating_`
- [ ] `cargo clippy -p xavier --all-targets --features ci-safe -- -D warnings`
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Tests listed above exist and pass (no `#[ignore]` except pdfium-backed ones in issue 10)

## Read first

- `src/retrieval/gating.rs` (RRF call ~line 543), `src/search/rrf.rs`, `src/retrieval/config.rs`
- `AGENTS.md` (fmt, clippy -D warnings, English comments, `thiserror` in libs, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- existing gating tests must pass unchanged; `src/context/**`
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
