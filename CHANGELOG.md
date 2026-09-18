# Changelog

All notable changes to **Xavier** are documented in this file in adherence to [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) standards and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Code-graph O1–O7 suite** (`code-graph/`, wave-24): unified honest-confidence rubric (`confidence.rs`), language registry (`language.rs`), blast radius + test gate (`query`), incremental IDs with content-hash reindex (`indexer`, `db.file_hashes`), token-budget queries (`budget.rs`), PageRank router (`rank.rs`, petgraph 0.8), contracts + architecture signals (`query`, `contracts.rs`).
- **MCP `codegraph_route` + `codegraph_gods` tools** (`src/server/mcp/tools_core.rs`): query routing with confidence margin/refusal/`est_tokens` and hub-node listing, 16 core tools total (was 12).
- **CLI `code situ`** (`src/cli/handlers/code.rs`): `git diff --name-only HEAD` → `symbols_for_files` → `test_gate`, with honest refusal on clean trees.
- **O4 hash parity + rewire table** (`code-graph/src/indexer/mod.rs`, `db/mod.rs`): `verify_hash_parity` reconciles `file_hashes` with disk on every `apply_paths`; `stable_id_rewrites` maps legacy `default`-project IDs to project-scoped IDs.
- **O1 rubric in retrieval + belief** (`src/retrieval/gating.rs`, `src/memory/belief_graph.rs`): `weights_from_confidence` / `o1_backfill` per ADR-032 with tie-down, legacy fallbacks preserved.
- **Parser dispersion arbitration** (`code-graph/src/parser/mod.rs`): deterministic winner + disclosed losers for multi-parser collisions.

### Fixed
- **Plugin tests hermetic** (`code-graph/src/plugin/`): 3 pre-existing env-dependent failures fixed via test seams (132+3 → 148 green, zero regressions).
- **Version sync drift**: `package.json` + `panel-ui/package.json` 0.2.3 → 0.2.4 (Version Sync Gate green); `build-version.test.ts` derives the version from `Cargo.toml` instead of hardcoding, build-test timeouts 30s → 120s.
- **Indexer fast-path**: known extensions skip the per-file plugin discovery probe in `parse_file` and `apply_paths` (output pinned by identity test 35/2470/13686).

### Security
- **CTEQ length-check DoS guards** (`src/crypto/hmac.rs`, `src/security/recovery.rs`, `src/server/headless/auth.rs`, `src/server/mcp/auth.rs`, `src/cli/http_setup.rs`): length check + dummy constant-time op before `ct_eq`, preventing `subtle` panics on attacker-controlled lengths.
- **Training path traversal queued**: `training.rs` joins raw dataset `id` to disk (`..` passes the char filter); scoped fix approved, `state.rs`/`session.rs` verified unaffected.

## [0.2.4] — 2026-09-17

### Added
- **True Hybrid RRF & Reciprocal Rank Fusion**: Full hybrid retrieval pipeline fusing BM25 lexical and vector embeddings with RRF rank scoring (`src/memory/qmd/search/hybrid.rs`).
- **Temporal Half-Life Decay**: Recency weighting in candidate scoring using exponential decay (`(-age_days / half_life_days).exp()`) in `src/memory/qmd/search/scoring.rs`.
- **TGD Path-Based Utility Heuristics**: Automatic categorization and low-utility scoring for ephemeral execution traces, periodic crons, and synthetic agent memory paths in `src/memory/tgd.rs`.
- **Autonomous Memory Pruner CLI**: New `xavier memory prune` CLI with `--dry-run`, `--yes`, `--older-than-days`, and category-level reporting (`src/cli/commands/memory.rs`).
- **Word-Boundary Smart Snippet Clipping**: Punctuation- and word-boundary aware search preview clipping avoiding mid-token cutoffs in `src/memory/snippet.rs`.

### Fixed
- **MCP Process Memory Duplication & Bloat**: Eliminated redundant eager memory document pool loading in `build_mcp_state` (`src/cli/mcp.rs`), preventing duplicate 40k+ document arrays and freeing ~5.5 GB RAM.
- **Corpus Garbage Collection**: Pruned 28,043 stale records (~77.5 MB) via TGD utility pruner, reducing working set size by 64% and stabilizing dual HTTP/MCP runtimes.
- **Resilient Store Decryption**: Graceful fallback and error recovery during workspace state migration with encrypted payloads.

### Security
- **Recovery secrets now Argon2id**: `hash_seed_phrase` / `hash_backup_code` (`src/security/recovery.rs`) emitted unsalted SHA-256. They now emit salted Argon2id PHC strings via `crypto::password`, with new `verify_seed_phrase` / `verify_backup_code` helpers that still accept legacy SHA-256 hex as a migration fallback. All callers migrated (CLI recovery handlers, `auth2` register/reset/2FA, `verify_and_consume_backup_code` now matches the raw code in Rust instead of by SQL equality).
- **FTS5 injection closed**: `search_code` (`src/codebase/db.rs`) and `search_logs` (`src/observability/service_log.rs`) passed raw user input to `MATCH`, turning FTS5 syntax into HTTP 500s. Both now sanitize through `sqlite_vec_store::fts::build_fts_query`; token-less queries return empty results.
- **Dependency vulnerabilities patched**: `h2` 0.4.15 → 0.4.19 (RUSTSEC-2026-0258), `ruint` 1.19.0 → 1.20.1 (RUSTSEC-2026-0220), `rustls` 0.23.42 → 0.23.45 (RUSTSEC-2026-0285). `rsa` 0.9.10 (RUSTSEC-2023-0071, no fix available) risk-accepted: usage is JWT RS256 sign/verify only, no PKCS#1 v1.5 decryption.

### Fixed
- **Nested Tokio runtime panic**: `VecSqliteMemoryStore::init_schema` built a runtime inside `block_in_place` (panics on current-thread runtimes, stalls the scheduler). Now runs the async init on a dedicated OS thread via `thread::scope`.
- **CodeGraph WAL corruption on recreate**: `CodeGraphDB::create_new` deleted the main file but left `-wal`/`-shm` sidecars behind (stale WAL → "disk image is malformed"). Sidecars are now removed too.
- **Stdio MCP no longer requires HTTP token**: `validate_stdio_connection` rejected local sessions without `XAVIER_TOKEN`. Local stdio inherits OS process trust (warn + allow); HTTP/SSE transports still enforce token/JWT strictly.
- **Mutex poisoning DoS**: `F12State` registry and `GoogleOAuthManager` used `std::sync::Mutex` with `.lock().unwrap()` in Axum handlers. Migrated to `parking_lot::Mutex` (already a dependency).
- **Verify pipeline accepts `in_progress`**: `scripts/verify-pipeline.sh` aborted on the `feat-classified-clearance` status; the status tuple (schema gate + test selection) now includes it. `--check-only` green: 59 features, 54 stable, score 91.1%.
- **Broken DX defaults**: removed `target-dir = "/tmp/xavier-target"` from `.cargo/config.toml` (docs say `./target/...`); excluded `panel-ui/src-tauri` (+ formal `xavier-core`) from the root workspace so headless/CI builds no longer require desktop GUI libs (also pruned ~800 lines of Tauri GUI deps from `Cargo.lock`); `.mcp.json` no longer references the nonexistent `gestalt-mcp` and uses repo-relative paths (`./scripts/mcp/xavier-mcp-cursor.sh`, no hardcoded `XAVIER_DATA_DIR_OVERRIDE` — the launcher auto-detects `REPO_ROOT/data`); `shell.nix` defaults to a local `./target` (`CARGO_TARGET_DIR="$(pwd)/target"`) instead of a forced `$HOME/.cargo/xavier-target` so nix-shell users find their binary at `./target/release/xavier`; `scripts/verify-pipeline.sh` is executable; repo-root runtime junk (`*.db*`, `*.o`, `patch_*`) removed (all gitignored, none tracked).

## [0.2.2] — 2026-09-14 (retrospective)

> **Note:** tag `v0.2.2` was cut on PR #2248 without a manifest bump (`Cargo.toml` / `package.json` stayed at `0.2.1`). This entry documents it retrospectively; the manifests jump directly to `0.2.3`.
- **Docker BuildKit optimization release** (#2248): BuildKit cache mounts for pnpm store, Cargo registry and target in `Dockerfile`; release binary copied to `/app/xavier` inside the builder stage to survive the target cache mount; explicit `HEALTHCHECK` on `http://127.0.0.1:8006/health`; `.dockerignore` extended to exclude build artifacts, test results, docs, scripts, agent files and sqlite caches.

## [0.2.1] — 2026-09-13

### Added
- **CodeGraph UDS Sidecar & Zero-Overhead IPC**: Native Unix Domain Socket client for fast symbol queries, automated installer routines, and process health telemetry.
- **Air-Gap Capsule Protocol & USB Storage Transport**: AES-256-GCM encrypted air-gap capsules with Argon2id KDF, integrity inspection, and auto-detection of removable USB volumes.
- **Multimodal Virtual Vector Manager**: Modal partition isolation (`MultimodalVecManager`) in `sqlite-vec` for unified multi-modal RAG across text, audio, images, and documents.
- **Maloca Introspection & Challenge Curation API**: REST endpoints for human challenge curation, Socratic introspection session logging, and training readiness verification.
- **Google OAuth2 & Colab Compute Uplink**: Authenticated flow for external compute access and decentralized SLM fine-tuning.
- **CodeGraph Walk Hardening**: Binary extension filtering without IO, 64 MiB `DEFAULT_MAX_FILE_SIZE` walk cap, and 8 KiB NUL byte header sniffing.

### Fixed
- **CI/CD Parallel Rust Test Stability**: Integrated 4 GB Linux swapfile allocation in GitHub Actions runner, preventing OOM exit code 137 on large test batches.
- **Code Formatting Compliance**: Synchronized codebase formatting with `cargo fmt --all`.
- **Frontend Test Suite Parity**: Validated 185 Vitest unit tests and 8 Playwright E2E suites passing cleanly.

### Marked for Upcoming Work Waves (Olas 9-11)
- **Ola Theme Redesign**: Theme token engine, color attenuation, and Studio Dark / Bone White visual modernization (`.gitcore/issues/wave-theme/`).
- **Sovereign Node Vault & WebAuthn PRF Bridge**: WebAuthn hardware-backed PRF extension integration and decentralized node login flow.
- **Sovereign Wallet & Merkle Rollup**: TopStatusBar Karma integration and Polygon payout Merkle rollup sync.
- **Disaster Recovery Wizard**: PWA mnemonic phrase backup quiz and restore wizard.

## [0.1.1] — 2026-09-02

### Fixed
- **WAL health pragmas** (#1793, #1801): `wal_autocheckpoint=1000` + `journal_size_limit=10485760` + opportunistic checkpoint on open if WAL ≥ 50MB.
- **Documentation i18n** (#1797): translated remaining Spanish to English across `docs/SRC/` and `docs/explanation/`.
- **Playwright E2E** (#1799): fixed `generative-ui.spec.ts` drift from OpenUI cockpit to XAVIER LOGIN flow.
- **Preflight docs** (#1798): updated README + QUICKSTART with `periferia/swal-preflight` repo-only usage (no npm).
- **KNOWN_ISSUES** (#1796): documented WAL 55MB remediation and verification steps.

## [0.1.0] — 2026-09-01

## [0.0.1] — 2026-08-30 (Initial Public Release)

### Added
- **Foundational Cognitive Memory Core**:
  - Multi-tiered memory architecture (Working, Epistemic, Episodic, and Procedural).
  - Fast vector embedding and hybrid search engine powered by `sqlite-vec` and RRF.
  - Native Model Context Protocol (MCP) server integration for AI agents (Hermes, OpenCode, Claude, Codex).
  - AST Code Graph indexing with semantic navigation and call-graph traversal.
- **Enterprise Mesh & Decentralized Synchronization**:
  - Secure peer-to-peer mesh replication with ed25519 cryptographic keypairs.
  - Role-Based Access Control (RBAC), multi-tenant isolation, and read-once ephemeral token passes.
  - Dynamic peer discovery via LAN broadcast, ICE/STUN NAT traversal, and Tor onion routing fallback.
- **Cross-Platform Multi-Architecture Releases**:
  - Automated release pipeline for Linux (x86_64, aarch64), macOS (Intel, Apple Silicon), and Windows (`xavier.exe`).
  - Official multi-architecture Docker container images published to GHCR.
- **Documentation & User Manuals**:
  - Comprehensive user guides, API reference, deployment architectures, and connected knowledge graph documentation located in `docs/`.
