# FEATURE: Codegraph → Maturity/Docs Bridge

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-29

## Overview
The Codegraph to Maturity and Documentation Bridge links AST-backed codebase intelligence directly with Xavier's downstream automated assessment pipelines (Maturity Scanning) and documentation engines (Chronicle Auto-Docs). By automating file exports and aligning paths, the bridge provides consistent access to codebase structure and prevents fallback degradation.

## Architecture & Design
The bridge is designed around three main layers:
1. **Shared Path Resolution (`src/codebase/codegraph_paths.rs`)**: Restructures paths so both SQLite databases and portable JSON dumps resolve identically relative to the workspace root, eliminating desynchronized database paths.
2. **Asynchronous Soft-Fail Exports (`src/cli/code_dump.rs`)**: Automatic trigger of `perform_dump` runs inside a background `spawn_blocking` task during HTTP or CLI indexing/scanning. Disk and serialization errors are caught safely, preventing dump write failures from interrupting primary indexing.
3. **Resilient Multilayer Maturity Chain (`src/maturity/scanner/code_graph.rs`)**: Evaluates symbol existence by falling back progressively across three distinct tiers (SQLite -> JSON dump -> multi-threaded grep fallback) to prevent scanner panics or scoring failures.

## Implementation Paths
- `src/codebase/codegraph_paths.rs` (shared path resolution helpers)
- `src/cli/code_dump.rs` (soft-fail portable JSON database export)
- `src/cli/handlers/code.rs` (asynchronous automatic dump hook on index/scan)
- `src/maturity/scanner/code_graph.rs` (multi-tier symbol scanning chain)
- `src/chronicle/auto_docs.rs` (auto-docs path alignment)

## Sub-features
- **cg-path-helper**: Single source-of-truth workspace path resolution for all databases and portable formats.
- **cg-auto-dump**: Auto-runs a background soft-fail dump of full graphs to `.xavier/codegraph.json` immediately after scan or index completion.
- **cg-maturity-chain**: Gracefully resolves anchors via a robust SQLite -> JSON -> grep chain.
- **cg-chronicle-align**: Realignment of chronicle CLI and auto-docs generators to point directly to the same database locations.

## Test References
- `maturity::scanner::code_graph::tests::test_scan_code_graph_empty_dump_falls_back_to_grep`
- `maturity::scanner::code_graph::tests::test_scan_code_graph_empty_sqlite_falls_back_to_grep`
- `cli::code_dump::tests::test_soft_perform_dump_never_panics`
- `tests/integration/codegraph_dump_test.rs`
- `chronicle::auto_docs::tests::test_render_module_doc_basic`

## Known Issues & Notes
- Heavy workspaces (>25k symbols) skip the auto-dump by default to avoid slow serialization stalls.
- Direct SQLite lookup is preferred when querying symbol details, achieving latency targets well below 10ms.
