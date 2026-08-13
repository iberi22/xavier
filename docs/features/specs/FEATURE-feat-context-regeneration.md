# FEATURE: Context Regeneration & Perfect Recall

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-20

## Overview
Context Regeneration and Perfect Recall implements an automated context assembly, evaluation, and verification pipeline. By processing real message sequences, it continuously evaluates and optimizes Recall@K and MRR (Mean Reciprocal Rank), dynamically adjusting working memory allocations and budget configurations to maximize retrieval accuracy under varying token limits.

## Architecture & Design
The context pipeline centers around `ContextRegenerationPipeline` which unifies:
1. **RegenerationLoop:** Detects session staleness and token growth ratio thresholds to auto-trigger context compaction.
2. **ContextIndexer:** Stores raw session message history in-memory for fast RRF hybrid querying.
3. **WorkingMemory:** Acts as a real-time, volatile cache of active agent context (bounded FIFO with LRU fallback).
4. **ContextBuilder:** Formats and compresses tiered context strings (extractive compression).
5. **Quality Evaluation Harness (ctx-recall-harness):** Measures Recall@K and MRR over queries using simulated/real session context and expected ground-truths.
6. **Auto-tuning Loop (ctx-auto-tune):** If measured recall is below a target, sequentially increments Orchestrator budgets (documents and token thresholds) until perfect recall (100%) is met.
7. **Extractive Episodic Summarizer (ctx-episodic-real):** Extracts key decisions, architectural events, and conversational flows to package past interaction histories.

## Implementation Paths
- `src/context/pipeline.rs` (Main ContextRegenerationPipeline and quality/tuning loops)
- `src/context/regen_loop.rs` (Staleness and growth check metrics)
- `src/memory/working.rs` (In-memory bounded cache)
- `tests/integration/context_regen_test.rs` (Full multi-turn dialogue, quality benchmark, and auto-tuning integration tests)

## Sub-features
- **ctx-recall-harness:** Fully implemented under `evaluate_recall`, calculating precise Recall@K and MRR against specified ground-truth mappings.
- **ctx-turn-pack:** Packages multi-turn conversation histories into tiered context blocks via `ContextBuilder`.
- **ctx-episodic-real:** Extractive episodic summarizer implemented under `summarize_episodic` extracting decisions and incident headings.
- **ctx-working-wire:** Volatile `WorkingMemory` dynamically integrated with incoming dialogue messages in `process_message`.

## Test References
- `context_regen_test::test_full_context_regeneration_pipeline_workflow` verifies auto-regeneration, metrics scoring, budget auto-tuning, and episodic summarization.
- `context::pipeline::tests` verify pipeline logic and episodic heuristics on mock datasets.

## Known Issues & Notes
- Context budgets are dynamically scalable, allowing self-correcting configuration shifts to satisfy strict quality guarantees.
