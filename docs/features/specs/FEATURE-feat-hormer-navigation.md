# FEATURE: HORMER Hierarchical Navigation

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
HORMER (Hierarchical Organized RAG Memory & Extraction Routing) implements a structured cognitive navigation framework. It utilizes a dynamic directory hierarchy, reinforcement learning (RL) scoring policies, Textual Gradient Descent (TGD) optimization, and GRPO to dynamically locate and organize memory clusters.

## Architecture & Design
Memory documents are indexed into a tree-like hierarchy. The navigation policy scores sub-trees to guide agents directly to relevant knowledge branches without needing to parse the entire database flatly. Auto-improvement is achieved through textual gradient feedback loops where semantic retrieval mismatches adjust node rankings.

## Implementation Paths
- `src/memory/hormer/` (HORMER nodes, navigation policies, and GRPO estimators)
- `src/navigation/` (tree traversal and navigation controllers)

## Sub-features
- **Dynamic Directory Hierarchy:** Organizes text chunks in cognitive directories.
- **Navigation Policy with Smart Scoring:** Evaluates relevancy weight of semantic nodes.
- **Textual Gradient Descent (TGD):** Auto-tuning and self-optimizing system prompts and weightings based on search hits.
- **GRPO Optimization:** Simplified reinforcement learning algorithm adjusting policy outputs.
- **Shell Commands (API + CLI):** Interface to manually traverse and check HORMER tree structures.

## Test References
- Navigation-aware consolidation tests.
- GRPO step update validation and scoring convergence tests.

## Known Issues & Notes
- Phase 2 Polish is frozen for Product 1.0 stability. HORMER is fully operational in production.
