# Xavier Changelog

## [1.0.0-rc.1] - 2026-05-16

### Added
- **Autonomous Integrator Protocol**: Systematic workflow for merging agent-generated PRs with automatic build verification.
- **Hierarchical Memory (L0-L1-L2)**: Stabilized multi-layered memory virtualization to minimize token consumption and stabilize context.
- **Token Budget Enforcement**: Hard token caps (4000 tokens) in context assembly pipeline with greedy relevance truncation.
- **Belief Graph Stabilization**: Deterministic graph traversal, contradiction detection, and high-performance BFS for relationship mapping.
- **XDG Compliance**: Implemented cross-platform path resolution using the `dirs` crate, supporting `XDG_CONFIG_HOME` and native data paths.
- **API Rate Limiting**: Integrated `rate_limit_middleware` in the HTTP server to protect against API abuse and track global usage.

### Changed
- **Production Readiness**: Hardened hexagonal architecture boundaries and removed legacy dead code across CLI and core layers.
- **Modular Memory Engine**: Refactored the monolithic `sqlite_vec_store` into specialized submodules for improved maintainability.
- **Security Hardening**: Integrated structured logging for sanitization and resolved outbound timeout issues. Added global API gateway rate limiting.

### Fixed
- **Memory Persistence**: Resolved hierarchical field initialization issues in `MemoryDocument` and `MemoryRecord`.
- **Concurrency Stability**: Fixed Rayon/Tokio deadlocks in high-performance modules.
- **Schema Integrity**: Fixed missing columns in `relations` and `timeline_events` tables with automatic migrations.
- **Vector Search Fixes**: Resolved `ON CONFLICT` compatibility issues with `sqlite-vec` virtual tables in `upsert_vector`.

## [0.4.0] - 2026-03-24

### Added
- **TUI Dashboard**: Interactive terminal-based monitor using `ratatui` for real-time memory and metrics visibility.
- **Git-Chunk Synchronization**: Decentralized sync protocol using compressed JSONL chunks for friction-less memory sharing via Git.
- **Local LLM Provider**: Native support for local OpenAI-compatible endpoints (Ollama, LocalAI) via `ModelProviderKind::Local`.
- **Hierarchical Curation**: Memory Manager categorizes facts using CurationAgent (Domain > Topic).
- **Temporal Graph**: Belief Graph now ingests `valid_from` timestamps connected to session context.

### Changed
- **Metadata Flexibility**: Memory documents now support arbitrary JSON metadata, fully queryable and displayed in the TUI.

## [0.3.0] - 2026-03-17

### Added
- **Security Audit**: Performed a comprehensive security review and documented findings in `security_audit_report.md`.
- **REST API Exposure**: Integrated Axum-based HTTP endpoints for memory search, addition, and agent runtime interaction.
- **Enhanced Retrieval**: Implemented hybrid search combining semantic embeddings with keyword-based retrieval.
- **Self-Improving Agents**: Added experimental `self_improve.rs` module for autonomous performance analysis and optimization.
- **Belief Graphs**: Operationalized `belief_graph.rs` to track relationships between memory nodes.

### Changed
- **Architecture Realignment**: Migrated repository structure to comply with Git-Core v3.2 Protocol.
- **Documentation Consolidation**: Centralized system specifications, research, and agent prompts under the `docs/` hierarchy.
- **Auth Middleware**: Standardized `X-Xavier-Token` enforcement across all public endpoints.

### Fixed
- **Docker Integration**: Resolved health check failures and port binding conflicts in the development stack.
- **Dependency Management**: Aligned crate versions for `axum`, `tokio`, and `surrealdb` across workspace members.

---

## [0.2.0] - 2026-03-08

### Added
- **Hybrid Retrieval Engine**: Initial implementation of the multi-stage memory search.
- **MCP Surface**: Added Model Context Protocol support for seamless IDE integration.
- **Persistence Layer**: Integrated SurrealDB as the primary storage engine for belief graphs.

---

## [0.1.0] - 2026-03-01

- **Initial Baseline**: Core Rust-native runtime for agent memory orchestration.
