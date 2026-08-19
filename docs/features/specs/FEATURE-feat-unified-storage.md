# FEATURE: Unified SQLite Storage

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
Unified durable storage using SQLite + `sqlite-vec` for high-dimensional vector search, graph data nodes, and memory persistence. This feature ensures that all cognitive components (vectors, relationships, episodic memories, and session states) are stored in a unified, local-first database structure with robust connection pooling.

## Architecture & Design
The storage layer is built around standard SQLite databases, leveraging connection pooling via `r2d2`. For vector capabilities, the system integrates `sqlite-vec` directly to perform efficient nearest-neighbor searches without needing external heavy databases.

## Implementation Paths
- `src/storage/` (base database connection and connection pooling setup)
- `src/memory/` (memory tables, memory records, vector inserts)
- `src/adapters/outbound/` (database adapters and repositories)

## Sub-features
- **Initialize SQLite + sqlite-vec:** Setup database schema and dynamically register the `sqlite-vec` extension at startup.
- **Store High-Dimensional Vectors:** Manage the `memory_embeddings` vector table for semantic queries.
- **Store Graph Nodes:** Store nodes and edges representing concepts and beliefs in the entity/belief graph.
- **Connection Pooling:** Use `r2d2` to manage thread-safe, concurrent database connections.
- **Migration System:** Dynamic schema versioning and schema migrations.

## Test References
- Integrated tests inside `src/storage/` and `src/memory/`.
- Concurrency and database connection robustness tests under load.

## Known Issues & Notes
- Columnar storage or manual `VACUUM` processes are considered optional polish and are kept out of the MVP.
- Performance on Windows is fully stable.
