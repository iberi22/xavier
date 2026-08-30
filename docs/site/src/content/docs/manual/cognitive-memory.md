---
title: Cognitive Memory Architecture
description: 4-layer cognitive memory, sqlite-vec, Reciprocal Rank Fusion (RRF) and hybrid search.
---

# Cognitive Memory Architecture

Xavier implements a 4-layer cognitive architecture:
1. **Episodic Memory**: Raw interaction session logs and trajectories.
2. **Semantic Memory**: Hybrid vector embeddings (`sqlite-vec`) + BM25 keyword matching with RRF scoring.
3. **Belief Graph**: Dynamic truth assertion network with confidence scores and contradiction detection.
4. **Procedural Memory**: Execution patterns, tool runbooks, and skills.
