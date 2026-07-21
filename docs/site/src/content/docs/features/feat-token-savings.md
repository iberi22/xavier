---
title: "Token Savings Progressive Disclosure"
description: "MemGPT-style index-first MCP search + page-in context + real memory layers for ~90% agent token savings"
---

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
A high-performance context retrieval pipeline designed to reduce token usage by ~90% for active LLM agents. Drawing inspiration from MemGPT, it introduces an index-first approach that separates metadata/snippet discovery from full-text content pagination, preventing excessive context-window inflation.

## Architecture & Design
The progressive retrieval flow consists of two stages:
1. **Fat Search:** The `mem_search` tool returns a compact list of metadata and snippets under a structured format, omitting full contents.
2. **Page-In:** If the agent needs details from a specific file, it invokes `memory_context(ids=[...])` to pull full text under a strict per-document token budget.
Token counts are computed using `estimate_tokens` (which performs scalar-level ceiling division `(char_count + 3) / 4` on Unicode values).

## Implementation Paths
- `src/server/mcp/tools_memory.rs` (the MCP `mem_search` and `memory_context` tools)
- `src/context/token_estimate.rs` (unified, non-split scalar token estimation helper)
- `src/memory/episodic.rs` (extractive session summaries to bound context)
- `docs/TOKEN_SAVINGS_ANALYSIS.md` (token utilization and savings report)

## Sub-features
- **ts-p0-mem-search-default/structured:** `mem_search` returns structured candidate arrays instead of flat, verbose texts.
- **ts-p0-memory-context-ids:** Retrieves full text only for explicit document IDs.
- **ts-p0-memory-context-per-doc-budget:** Binds context pagination sizes strictly to prevent window overflow.
- **ts-p1-unified-estimate-tokens:** Replaces custom string/whitespace token counts with `estimate_tokens`.
- **ts-p3-episodic-summaries:** Binds session summaries below ~400 tokens using extractive keyword algorithms.
- **ts-p8-measurement-script:** Measuring script located at `scripts/measure_token_savings.py`.

## Test References
- MCP progressive disclosure suite integration tests in `src/server/mcp/progressive.rs`.
- `estimate_tokens` correctness and edge case unit tests.

## Known Issues & Notes
- Fully compliant with official MCP clients; verified by integration tests to ensure that full content is omitted from search unless explicitly requested.

### Functional Token Savings Example
Implement MemGPT-style progressive retrieval to cut context-window overhead:

1. **Fat Search (Retrieve metadata and snippets only):**
```bash
curl -X POST "http://localhost:8006/memory/search" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "recursive retrieval parameters",
    "include_content": false
  }'
```

2. **Page-In (Retrieve selected full contents when required):**
```bash
curl -X POST "http://localhost:8006/memory/context" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "ids": ["doc_908", "doc_912"],
    "max_chars": 5000
  }'
```
