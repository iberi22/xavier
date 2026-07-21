---
title: "Context Regeneration & Perfect Recall"
description: "Continuous context regeneration using real usage data to drive recall@k toward 100% on production benchmarks"
---

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
Context Regeneration and Perfect Recall implements an automated context assembly and verification pipeline. By processing real message sequences, it evaluates and optimizes Recall@K and MRR (Mean Reciprocal Rank), rebuilding working memories to maximize retrieval accuracy under varying token limits.

## Architecture & Design
The context pipeline relies on a dedicated `WorkingMemory` which acts as a real-time, volatile cache of active agent context. If a query requires deeper details, the "turn pack" virtualizes the conversational history, performing targeted episodic retrieval to pull relevant past turns and stitch them together using unified token estimations.

## Implementation Paths
- `src/context/` (WorkingMemory structures and turn packaging)
- `src/memory/` (episodic retrieval, candidate scoring, and context builders)

## Sub-features
- **ctx-recall-harness:** Measures Recall@K on production benchmarks and flags quality degradation.
- **ctx-turn-pack:** Packages multi-turn conversation histories into tiered context blocks.
- **ctx-episodic-real:** Extractive episodic summarizer that runs automatically on message stores.
- **ctx-working-wire:** Direct integration of the volatile `WorkingMemory` with active agent completion steps.

## Test References
- Working memory updates and episodic retrieval precision tests.
- Recall harness correctness and score tracking unit tests.

## Known Issues & Notes
- Bounded to 100 most recent documents (the hot set) during retrieval steps to prevent full database dumps.
- Context compression algorithms are fully functional, with subsequent polish added incrementally.

### Functional Context Regeneration Example
Regenerate agent context using Turn Packs to maintain peak semantic recall:

```bash
curl -X POST "http://localhost:8006/v1/context/regenerate" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "session_id": "sess_91823",
    "compression_mode": "extractive",
    "target_token_budget": 2048
  }'
```
