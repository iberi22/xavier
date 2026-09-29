# [PAGEINDEX.15] Follow-up: node-summary vector leg for the hybrid arm

> Wave pageindex — F3 follow-up | Feature: `feat-pageindex-hybrid` | ADR-036 | Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`
> Status: **planned** (not part of this wave) | Depends on: 13 | Risk: HIGH (touches gating input and the memory embedding path) | Effort: Large 4-5h
> Worktree/branch: dedicated `feat/pageindex-15` from main once the wave has merged. One agent, one session.

---

## Current State (MEDIBLE)

- Issue 13 shipped the BM25 node leg in `src/retrieval/pageindex_arm.rs`: BM25 over node title + summary + own page text, cached per workspace, fused as one weighted RRF source in `AdaptiveGating`, OFF by default (`XAVIER_PAGEINDEX_ARM_ENABLED`), fail-open. There is no vector leg and the Xavier memory store holds no node embeddings.
- Why BM25 first: the spec's E2E evaluation shows search-first navigation reaches 60/62 correct at 1.7 pages read per question (vs 11.6 without in-document search), so a lexical leg already carries most of the practical value at no embedding cost. The fixture eval (hit@3 0.778 for the arm vs 0.917 for DocBot 512-token chunks; paraphrased questions are hard for a lexical-only leg) is the evidence that a vector leg is worth testing.

## Desired State (DELTA)

1. `embed_document(doc)` persists each node summary (fallback: title + first 300 chars) through the existing memory embedding API with tags `pageindex`, `doc:<name>`, `node:<id>` and metadata `{start_page,end_page}`.
2. Re-ingest of a document replaces its stale node records (no orphans after edits or delete).
3. `search` queries ONLY pageindex-tagged records, workspace-scoped, and fuses the vector ranking with the existing BM25 node ranking (RRF) BEFORE the result enters the gating RRF as a single arm source.
4. Fail-open: any embedding/store error is logged and the arm degrades to the BM25 leg; the disabled path stays byte-identical.
5. The eval gains a hybrid+vector column; numbers are recorded in the spec. Ship the leg ON only if it improves hit@3 over BM25-only arm (0.778) on the fixtures.

## Files to Modify

| File | Change | Risk |
|------|--------|------|
| `src/retrieval/pageindex_arm.rs` | vector leg + leg fusion | HIGH |
| `src/pageindex_glue/settings.rs` | leg toggle (e.g. `XAVIER_PAGEINDEX_ARM_VECTOR`, default false) | HIGH |
| `.env.example` | +1 var | HIGH |
| `tests/pageindex_eval.rs` | hybrid+vector column | MEDIUM |
| `docs/features/specs/FEATURE-pageindex-tree-retrieval.md` | record measured numbers | LOW |

## Tests that prove it

- `test_arm_vector_leg_persists_node_summaries_with_doc_node_page_tags`
- `test_arm_vector_leg_queries_only_pageindex_records`
- `test_arm_vector_leg_reingest_replaces_stale_nodes`
- `test_arm_vector_and_bm25_legs_fused_before_gating_rrf`
- `test_arm_vector_leg_workspace_scoped`
- `test_arm_vector_leg_failure_falls_back_to_bm25`
- `test_eval_hybrid_vector_column`

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo test -p xavier --lib --features ci-safe retrieval::`
- [ ] `cargo test -p xavier --lib --features ci-safe -- test_arm_ test_gating_`
- [ ] `cargo clippy -p xavier --all-targets --features ci-safe -- -D warnings` and default features
- [ ] Existing `test_gating_*` and BM25 `test_arm_*` tests unchanged and green
- [ ] `git status --porcelain` lists ONLY the files in the table above
- [ ] Spec records the measured hybrid+vector column; the leg defaults ON only if hit@3 beats the BM25-only arm

## Read first

- `src/retrieval/pageindex_arm.rs`, `src/retrieval/gating.rs` (RRF call), `src/search/rrf.rs`
- the memory embedding API used by `src/memory/` (verify symbols with grep before use)
- `AGENTS.md` (fmt, clippy -D warnings, English comments, no hardcoded env, Rayon/Tokio rule)

## DO NOT touch (Anti-Regression)

- `.gitcore/features.json` — status promoted only by a green `scripts/verify-pipeline.sh`, never by hand.
- `src/context/**`, `src/server/mcp/{mod,server,tools_core}.rs`.
- Existing gating behaviour when the arm is disabled.

## Anti-Hallucination Guard

1. READ before write; verify every referenced symbol with grep.
2. No invented crates; keep default/`ci-safe` free of native libs.
3. New `std::env::var` => add key to `.env.example` in the same PR (read only in `pageindex_glue/settings.rs`).

## PR Delivery Requirements

- [ ] 1 PR = this issue only; conventional commit `feat(pageindex): ...`; no co-author/AI trailers.
- [ ] If blocked, do not open an empty PR; comment the blocker on the issue.
