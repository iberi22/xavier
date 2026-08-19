# FEATURE: Belief Graph

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
The Belief Graph maps concepts and their semantic relationships, serving as an in-memory knowledge representation. It is designed to support conceptual relationship mapping, cognitive inference, and RAG-based context enrichment.

## Architecture & Design
The `EntityGraph` represents knowledge using nodes (entities) and edges (relations). To prevent quadratic complexity $O(N^2)$ bottlenecks during semantic extraction, candidate entities are pre-normalized and stored in a hash map for $O(1)$ lookups. The graph undergoes continuous decay to lower concept scores over time unless they are actively reinforced. Thread safety is achieved by using separate, sequential lock scopes on the inner `RwLock<GraphData>`.

## Implementation Paths
- `src/memory/entity_graph/` (graph definitions, extraction, indexing, and decay)
- `src/domain/belief/` (belief definitions and schemas)

## Sub-features
- **Define Relationship Nodes & Edges:** Standardized structures to map entities and their semantic links.
- **Implement Extraction Logic:** Highly optimized entity extraction using pre-normalization and hash map lookups.
- **Inference & Decay Mechanisms:** Ebbinghaus forgetting curve decay with the decay factor strictly clamped to `[0.0, 1.0]` to avoid mathematical instability.
- **JSON/Bincode Serialization:** Robust snapshot persistence to SQLite or file systems.
- **EntityGraph Snapshot Durability:** Automatic reloading and saving of graph state on startup and mutation.

## Test References
- `test_concurrent_graph_operations` and `test_concurrent_graph_operations_heavy` verifying thread safety and deadlock-free concurrent executions.
- `src/memory/entity_graph/storage.rs` unit tests for clamped decay factors.

## Known Issues & Notes
- Entity graph indexing is delegated to a background task (`tokio::spawn`) since 2026-06-16 to avoid blocking the main execution path.
- Snapshot durability has been verified, and graph data is exposed via Memory KG HTTP endpoints.
