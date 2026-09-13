# Changelog

All notable changes to **Xavier** are documented in this file in adherence to [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) standards and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
