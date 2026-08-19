# FEATURE: BM25 + Vector Hybrid Search

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-14

## Overview
Hybrid Search combining BM25 keyword matching via SQLite's FTS5 engine and high-dimensional vector retrieval with Reciprocal Rank Fusion (RRF) merging to achieve high-recall, precise semantic results.

## Architecture & Design
The search orchestrator issues parallel queries to:
1. SQLite FTS5 index for BM25 text relevance.
2. `sqlite-vec` index for vector similarity.
The results are then merged using Reciprocal Rank Fusion (RRF), sorted alphabetically in case of ties on document ID, and deduplicated by path, preserving the most recent update.

## Implementation Paths
- `src/search/` (BM25 keyword search, FTS5 schema, and RRF implementation)
- `src/retrieval/` (retrieval orchestrators)
- `src/embedding/` (embedding managers and providers)

## Sub-features
- **Implement BM25 Full-Text Search:** Utilizes SQLite FTS5 for swift text matching.
- **Implement Vector Search:** Queries vector embeddings via the `sqlite-vec` virtual tables.
- **Implement Reciprocal Rank Fusion (RRF) Merging:** Computes combined rank scores for hybrid results.
- **Embedding Provider Integration:** Bridges cloud and local embedding providers (including OpenRouter and Ollama).
- **Caching Layer:** High-performance LRU caching layer for repeated queries to optimize latency.

## Test References
- `src/search/rrf.rs` tests for combining/scoring hybrid search results, alphabetical tie-breakers, and deduplication.
- Hybrid search integration tests in retrieval modules.

## Known Issues & Notes
- Local embedding fallback on AMD GPUs is currently missing/under development.
- "Fat Search" pattern is implemented via Progressive Memory Disclosure (the `mem_search` tool returns metadata and snippets by default, and `include_content: true` or `memory_context(ids=[...])` is used to page-in the full text).
