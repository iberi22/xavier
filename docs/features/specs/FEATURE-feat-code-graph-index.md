# FEATURE: Code Graph Index

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
AST-backed code indexing and symbol search through the specialized `code-graph` sidecar crate. The code graph reads codebase files, extracts syntax trees using `tree-sitter`, and maps symbols and relationships to an FTS5-enabled SQLite index.

## Architecture & Design
The code graph indexes functions, classes, traits, methods, and modules. Symbol indexing utilizes an FTS5 virtual table for lightning-fast keyword lookup. The UI accesses these relationships via `/code/graph/view` which outputs a force-directed layout representation (overview or ego modes) for a rich multi-layer visualization.

## Implementation Paths
- `code-graph/` (the entire AST indexer sidecar and symbol database)
- `src/codebase/` (Xavier interface and CLI handlers for scanning)

## Sub-features
- **cg-ast-multi-lang:** Abstract Syntax Tree parsing and relationship extraction for multiple languages (Rust, Python, TS, JS, etc.) using `tree-sitter`.
- **cg-http-api:** Exposes REST endpoints (`/code/scan`, `/code/find`, `/code/stats`, `/code/graph/view`) to fetch symbol maps.
- **cg-fts5:** Implements FTS5 text search inside the symbol index to quickly locate specific functions or terms.

## Test References
- Code graph multi-language indexer unit and integration tests.
- `cargo test --bin xavier test_map_edges_to_graph` for edge mapping verification.

## Known Issues & Notes
- Built artifacts are managed properly; any plugins registry index uses the `XAVIER_PLUGIN_REGISTRY_URL` environment variable, defaulting to a local JSON fixture.
- Force-graph payload endpoints are mapped cleanly into the frontend UI under the Code visualization layer.
