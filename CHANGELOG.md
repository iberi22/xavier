# 📜 Changelog — Xavier Cognitive Memory & Enterprise Mesh Runtime

All notable changes to **Xavier** are documented in this file in adherence to [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) standards and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [1.0.0] — 2026-08-30 (Official Production Release)

### 🌟 Highlights & Major Architectural Milestones

Xavier v1.0.0 is the foundational production release of the decentralized cognitive memory engine and peer-to-peer enterprise mesh network designed for autonomous agent swarms (Hermes, Gestalt, Jules) and secure human-in-the-loop collaboration.

---

### 🌐 1. Enterprise Mesh Networks & P2P Transport (Wave 16)
- **Multi-Node Mesh Topology**: Dynamic peer discovery, bidirectional manifest synchronization, gzip chunk export/import (`/v1/mesh/chunks/push`, `/v1/mesh/chunks/request`), and eventual dataset convergence.
- **P2P NAT Traversal**: RFC 5389 STUN message negotiation and RFC 8445 ICE candidate pairing (Host, ServerReflexive, Relay UDP/TCP).
- **Onion & Darknet Routing**: Native URI parsing and tunneling format for Tor hidden services (`.onion:8006`).
- **Resilient Offline Fallback**: Persistent SQLite `OfflineQueue` for disconnected nodes with automatic exponential backoff, jitter, and delivery upon peer reconnection.

### 🔐 2. Private Mesh & Wallet Cryptographic Isolation
- **Wallet-Gated Clusters**: `WalletId` derivation from Ed25519 public keys (`derive_wallet_id`), isolating private sub-meshes from unauthorized external peers.
- **Session Encryption**: End-to-end symmetric encryption of private memory deltas and snapshots using AES-256-GCM session keys (`encrypt_session_payload` / `decrypt_session_payload`).
- **Cross-Wallet Rejection**: Cryptographic fail-fast verification denying rogue or cross-tenant nodes access to private payloads.

### ⏳ 3. Ephemeral Data Packets & Clinical Access Passes
- **Read-Once Passes**: Single-use clinical and emergency access tokens (`read_once: true`) that automatically self-revoke upon first access (HTTP 403 on subsequent attempts).
- **Time-Locked TTL Passes**: Consultation access tokens with bounded temporal validity (`consultation_ttl`).
- **Secret Lending Engine**: Ephemeral credential leasing managed by `KeyLendingEngine` with active lease tracking and automated TTL expiration.

### 🏛️ 4. Synchronized DAO Governance & Security Protocols
- **Decentralized DAO Governance**: Democratic 1-node-1-vote proposal lifecycle (`/v1/mesh/dao/proposals`), vote casting with badge endorsements (`/v1/mesh/dao/proposals/{id}/vote`), and quorum threshold calculation.
- **KillSwitch Protocol**: Cryptographic broadcasting of `KillSwitchNotice` to permanently purge compromised peer nodes (`purged: true`) while guaranteeing root/master host immunity.
- **Multi-Tier Clearance & RBAC**: Granular clearance enforcement (`Unclassified`, `Internal`, `Restricted`, `Confidential`, `Secret`, `TopSecret`) and namespace access matrices.

### 🧠 5. Graph-Connected Cognitive Memory Core (Waves 14 & 15)
- **Code Graph Engine**: AST-based code symbol extraction, cross-repository import mapping, caller/callee relation graphs, and symbol link deduplication.
- **Entity & Epistemic Belief Graphs**: Persistent node-and-edge graphs in SQLite capturing entities, temporal beliefs, confidence weights, and belief revisions across agent sessions.
- **Bounded Working Memory**: Sliding-window short-term context bounded by token and document caps, rehydrating automatically into durable long-term memory.
- **Hybrid Retrieval Engine**: Reciprocal Rank Fusion (RRF) combining dense vector embeddings (OpenAI, Voyage, Ollama, Google GenAI) and lexical BM25 search.
- **Centralized Database Pragmas & WAL Streamer**: Automated checkpointing, 256MB mmap memory caches, and resilient recovery pipelines.

### 📦 6. Multi-Platform Binary Matrix & Release CI
- **Automated Multi-Arch Releases**: Cross-compilation GitHub Actions workflow generating native release binaries with SHA-256 checksums for:
  - Linux `x86_64-unknown-linux-gnu`
  - Linux ARM64 `aarch64-unknown-linux-gnu`
  - macOS Intel `x86_64-apple-darwin`
  - macOS Apple Silicon `aarch64-apple-darwin`
  - Windows `x86_64-pc-windows-msvc` (`xavier.exe`)
- **Container Registry**: Multi-arch Docker images published to GitHub Container Registry (`ghcr.io/iberi22/xavier:1.0.0` and `:latest`).

---

## [0.14.0] — 2026-08-25 (Pre-Release / Wave 15 Graduation)
- Graduated 46/46 core features with 100% test coverage.
- Centralized SQLite database pragmas across all stores.
- Implemented WAL auto-checkpointing and streaming backup handlers.
- Refactored Doctor diagnostic subsystem and memory query unification.

---

## [0.1.0] to [0.13.0] — 2026-05-24 to 2026-08-15 (Foundation & Evolution)
- Initial hexagonal architecture, memory store traits, and MCP protocol integration.
- AST parsing for code graphs and semantic vector store integrations.
- Multi-workspace tenancy, audit logs, and token rate limiting.
