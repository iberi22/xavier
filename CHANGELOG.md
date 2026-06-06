# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.1-beta] - Unreleased

### Meta — SWAL Maturity Sprint

Iniciando el Quality & Refactor Sprint para llevar el codebase a estándares Rust 2025:
- Archivos <500 lines (ideal: 100-200)
- Cobertura de tests >70%
- Cero clippy warnings
- Doc comments en TODO el código público
- CI con nextest + cobertura

#### Ronda 1 — Completado
- PR #504 — Refactor archivos >1000 lines en módulos pequeños (merged)
- PR #507 — Dependabot rust-minor bumps (merged)
- PR #510 — Split handlers.rs en 11 submódulos (merged)
- PR #511 — Cleanup .bak files + doc comments (merged)

#### Ronda 2 — Split archivos >1000 lines
- [Pendiente] Split src/settings.rs (1139 lines)
- [Pendiente] Split src/coordination/message_bus.rs (1268 lines)
- [Pendiente] Split src/cli/commands.rs (1145 lines)
- [Pendiente] Split src/memory/manager.rs (1117 lines)
- [Pendiente] Split src/memory/entity_graph.rs (1102 lines)
- [Pendiente] Split src/memory/qmd/search.rs (1045 lines)
- [Pendiente] Split src/agents/provider.rs (1016 lines)

#### Ronda 3 — CI y calidad
- [Pendiente] Fix 19 pre-existing clippy warnings
- [Pendiente] Migrate to cargo-nextest
- [Pendiente] Add coverage threshold (70%) to CI
- [Pendiente] Add pre-commit hook (rustfmt + clippy)

#### Ronda 4 — Documentación y tests
- [Pendiente] Add module-level doc comments to 184 files
- [Pendiente] Remove allow(dead_code) directives
- [Pendiente] Fix 4 pre-existing test failures
- [Pendiente] Add unit tests to 20 untested modules

### Added

- **Context Regeneration** — Multi-phase context regeneration system (Phase 0, 1, 2)
  - Phase 0: Core framework for context regeneration
  - Phase 1: Full context regeneration with session management
  - Phase 2: Enhanced regeneration with reconciliation
- **Multi-Provider Agent Spawn** — Support for MiniMax and DeepSeek providers with different context sizes (#96)
- **Agent Skill Context Loading** — Per-spawn skill context loading for agents (#97)
- **CLI-Based Agent Spawn** — Spawn 10-15 agents with provider routing (#98)
- **WebSocket Streaming** — Real-time event streaming via WebSocket
- **Hook System and BM25 Hybrid Search** — New hook system with BM25 hybrid search capabilities
- **Unified MCP Implementation** — Gestalt MemoryFragment tools with unified output formats
- **Dockerfile** — Sevier2 runtime environment defaults added to Dockerfile
- **Changelog** — CHANGELOG-MAY2026 documentation

### Fixed

- **Security Hardening** — Multiple security fixes applied:
  - Fixed SSRF risk in `verify_save_handler` and other outbound call sites
  - Aligned token generation and validation logic
  - Phase 1 Security Hardening with 4 critical fixes applied
  - Security Hardening documentation and regression tests
- **Test Fixes** — Resolved 19 failing tests
- **Compiler Errors & OOM** — Fixed compiler errors and out-of-memory issues (May 2026)
- **SessionSyncTask** — Made thresholds configurable via environment variables (#152)
- **AutoVerifier** — Fixed score to satisfy 0.8 threshold (#153)
- **Magic Constants** — Extracted magic constants to configuration in `gating.rs` (#154)
- **Error Handling** — Replaced `unwrap`/`expect` with proper error handling in `http.rs` (#156)
- **Router Wiring** — Wired `verify_save_handler` to CLI router (`cli.rs`) (#157)
- **Index Lag** — Implemented real `estimate_index_lag` in `SessionSyncTask` (#158)
- **Integration Tests** — Implemented real test logic in Sevier2 integration tests (#151)
- **MCP Wiring** — Wired MCP to `MemoryQueryPort`, removing TODO stubs (#161)
- **Agent Registry** — Wired agent registry through `AgentLifecyclePort` (#94, #159)
- **Agent Unregister** — Wired agent unregister endpoint as POST
- **Graceful Shutdown** — Added graceful shutdown for session sync cron
- **Duplicate Handler** — Removed duplicate session event handler
- **Hexagonal Architecture** — Untangled ports/infra layers (#90)
- **Docker Tests** — `cargo fmt` applied
- **Sevier2 Payload** — Corrected agent registration payload

### Documentation

- **Architecture** — Multi-crate workspace evolution strategy documented
- **Agent Pipeline** — Multi-agent development pipeline guide added
- **Code Reviews** — Architecture analysis and code reviews added
- **Jules Prompts** — Documentation for Phase 1-3 (8 complete tasks) added
- **Jules Workflow** — Jules workflow guide and daily memory added
- **Issue Updates** — Updated Jules prompts with new issue numbers
