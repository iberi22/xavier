# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0-rc.1] - Unreleased

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
