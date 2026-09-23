# Changelog

All notable changes to **Xavier** are documented in this file in adherence to [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) standards and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.6] — 2026-09-23 — stability wave S1

Measured outcome: ingestion-loop embedding rate 439-502 → 4 emb/min, cache hit_rate 41% → 88.4% (12.351 entries), GPU junction back to idle, NRestarts 0. Production binary rebuild pending maintainer approval; fixes verified in CI (fmt + clippy `-D warnings` + lib/integration tests green, incl. negative controls).

### Added
- Wave skill-injection: 6 planned issues (301-306) and ledger scaffolding.
- **Stability watch script** (`scripts/xavier-stability-watch.sh`, #2493): one-command KPI corroboration (emb/min, hit_rate, restarts, GPU, DB integrity), read-only.
- **FTS health reporting** (`src/observability/health.rs`, #2498): additive `database.fts_ok` probe; `/health` no longer certifies a corrupt FTS index as healthy.
- **Stability env docs** (`.env.example`, #2496): `XAVIER_INGESTION_INTERVAL_SECS`, `XAVIER_BUS_QUOTA_*` documented; TTL naming drift resolved (`XAVIER_EMBEDDING_CACHE_TTL` reads first, `TTL_HOURS` fallback).
- **Ingestion single-flight guard** (`src/cli/server.rs`, #2497): overlapping full-corpus cycles are skipped with a warning.
- **Cross-importer stability tests** (`tests/stability_ingestion_tests.rs`, #2505): 5/5 — second identical pass performs zero re-encodes, incl. shared-store cycle.
- **Cache persist roundtrip test** (`src/embedding/cache.rs`, #2505): 15/15.

### Changed
- **Storage PRAGMA layering** (`src/storage/pragma.rs`, `src/codebase/connection_manager.rs`): pooled connections now only re-apply the cheap per-connection pragmas (`busy_timeout`, `foreign_keys`) on acquire; the heavy settings are applied once per connection at pool construction and the opportunistic WAL checkpoint moved off the acquire path. This removes the `wal_checkpoint(TRUNCATE)` stalls that could block every checkout behind a long-running read.
- **Workspace store caching** (`src/storage/multi_db.rs`): `MultiDbManager::get_store` now returns a clone of the already-initialised store instead of re-opening the database file and replaying the migration set on every call; entries are invalidated by `delete_database`.
- **Idempotent migration bookkeeping** (`src/storage/mod.rs`): migration versions are recorded with `INSERT OR IGNORE`, so two concurrent initialisations of the same database no longer fail with a `schema_migrations` primary-key violation.

### Fixed
- **HermesImporter incremental skip** (#2476): identical content reuses the stored record — second identical pass performs zero `encode` calls (regression-tested, incl. JSON path and exactly-once re-embed on change).
- **Bus quota precheck** (#2477): read-only `bus_quota_exhausted()` peek skips GPU work the writer would drop; quota tests made order-independent.
- **Ingestion interval env** (#2478): `XAVIER_INGESTION_INTERVAL_SECS` (default 600, 0 disables the loop).
- **Importer skips for Antigravity/OpenCode/Codex** (#2505, S1.06/07/08): same incremental pattern, second-pass tests each.
- **Cache read-path lazy open** (`src/embedding/cache.rs`, #2505, S1.11): `try_lookup_sqlite` now opens an existing backing DB — previously the first lookup after every restart missed and re-embedded (caught by the new roundtrip test, 51-vs-50 without the fix).

### Known issues
- **FTS5 malformed index** (#2479, #2494): `malformed inverted index for FTS5 table main.memory_fts` recurred ~146×/day; `quick_check` stays `ok` (index-only corruption). Rebuild procedure specified and rehearsed on a copy — repair on prod requires fresh backup + maintainer approval.

## [0.2.5] — 2026-09-20

### Added
- **Telecom & Mesh Communication Runtime** (`src/telecom/`, Wave 26 & 27): Full peer-to-peer and relayed encrypted communication layer for multi-agent swarms.
  - X25519 / ChaCha20-Poly1305 double-ratchet session encryption (`crypto.rs`).
  - Wire framing and zero-copy packet serialization (`protocol.rs`).
  - Peer session state management with keep-alive beacons and heartbeat monitoring (`session.rs`).
  - Direct routes and fallback mesh relay routing (`routing.rs`).
  - Role-based clearance gate with Ed25519 wallet signature verification (`clearance_gate.rs`).
  - Chunked file transfer engine with SHA-256 verification and dropped chunk recovery (`file_transfer.rs`, `chunk_assembler.rs`).
  - Multi-party encrypted group chat rooms with epoch key rotation (`group_rooms.rs`).
  - Cognitive agent responder binding with policy evaluation triggers (`agent_responder.rs`).
  - Ephemeral message auto-cleanup worker (`ephemeral_cleanup.rs`).
  - NAT hole punching with STUN fallback (`nat_punch.rs`).
  - LZ4 payload compression with auto-thresholding (`payload_compression.rs`).
  - Sliding-window rate limiting (`rate_limiter.rs`).
  - Structured security audit logger for clearance rejections & signature verification (`audit_logger.rs`).
  - Native Prometheus metrics exporter (`metrics.rs`).
  - Stdio & HTTP MCP telecom tools integration (`src/server/mcp/telecom_tools.rs`).
  - Full CLI interface `xavier telecom` (listen, send, ping, rooms).
- **Panel UI Telecom Suite** (`panel-ui/`):
  - Multi-channel container `TelecomHubView` and 1-to-1 encrypted chat `DirectChatView`.
  - Group room viewer with topic editor and roster `GroupRoomView`.
  - Security timeline and audit view `TelecomAuditTrailView`.
  - Interactive file transfer drawer with progress and SHA-256 validation `FileTransferDrawer`.
  - Node latency & ping status monitor `PeerDiscoveryCard`.
  - Clearance level badge and classification visualizer `ClearanceBadge`.
  - Voice note audio player with waveform visualizer `VoiceNotePlayer`.
  - Ephemeral message toggle `EphemeralMessageToggle`.
  - Agent responder selector `AgentResponderSelect`.
  - Real-time WebSocket hook with exponential backoff `useTelecomSocket`.
  - Reactive room store hook with optimistic updates `useTelecomRooms`.
  - Streaming agent responder hook `useTelecomAgentResponse`.
- **Jules Multi-Account Autonomous Protocol v2.0**:
  - Centralized multi-account orchestration via dual keys (`JULES_API_KEY` & `JULES_API_KEY_2`).
  - Mandatory 3-phase internal execution workflow per issue (Types -> Core Logic -> Isolated Tests & PR Delivery).
  - Anti-Ambiguity / anti-`"?"` unblock rule preventing Google Labs VM sandbox resets and timeouts.
  - Automated blocker recovery through web research and findings documentation.
  - Autonomous unblocking daemon `jules-auto-unblock.py`.

### Fixed
- **Constant-Time Equality DoS**: Applied constant-time token comparison across CLI memory handlers and headless endpoints.
- **UI Reactivity**: Memoized notification center callbacks and `KarmaEventItem` in `WalletView`.
- **Indexer Reliability**: Resolved silent directory drop on backup folders during full-repo AST indexing.
- **Version Sync**: Enforced automated preflight gate across `Cargo.toml`, `package.json`, and `panel-ui/package.json`.

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
