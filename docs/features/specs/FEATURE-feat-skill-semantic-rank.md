# FEATURE: Semantic Skill Ranking via Local Embeddings

**Status:** `planned` | **Score:** — | **Last Tested:** —

## Overview
Skill matching today is `score_skill_match` (`src/context/skill_registry.rs:200-235`): keyword substring counting, no semantics. This feature ranks by cosine similarity over locally computed embeddings (existing `EmbeddingPort`, `src/memory/embedder.rs`, `sqlite-vec` store from `feat-unified-storage`), keeping the keyword score as tiebreak and emitting a calibrated 0–1 confidence consumed by fusion (#304) and MCP (#305). 100% offline: no cloud calls anywhere in the ranking path.

## Architecture & Design
- At `index_skill_file` time, embed `name + first 200 chars of description`; vectors persist in the existing `sqlite-vec` store under a skills namespace (no second DB).
- `search()` = cosine top-K → keyword tiebreak → confidence = clamped cosine. Empty vector store → keyword-only fallback (fail-open, existing behavior).
- Eval harness (location per repo test layout) with ≥20 labeled `query → skill` pairs; gates: Recall@3 ≥ 0.8 + reported MRR. Eval uses a mocked port: zero network in tests.
- CPU-heavy loops respect the Tokio+Ryon golden rule (`spawn_blocking` from async paths).

## Implementation Paths
- `src/context/skill_registry.rs` (embed-at-index, cosine rank)
- `src/memory/embedder.rs` / port impl (only if a new port method is required; reuse preferred)
- Eval harness (new file per repo test layout)

## Sub-features
- **Embed-at-index:** vectors alongside `IndexedSkill`.
- **Cosine top-K + tiebreak:** deterministic ordering.
- **Offline eval:** Recall@3, MRR, calibration-range test, no-network test.

## Test References
- `test_semantic_rank_recall_at_3`, `test_confidence_calibration_range`, `test_ranking_offline_no_network`, keyword-fallback test on empty store.

## Known Issues & Notes
- Embedding model stays `nomic-embed-text`; model selection is out of scope. Confidence must derive from measured cosine, never constants.
