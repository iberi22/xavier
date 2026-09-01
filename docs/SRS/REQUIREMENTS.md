# Software Requirements Specification — xavier

> **Protocol:** GitCore 3.8.0 · **Updated:** 2026-08-31
> IEEE 830 reduced. Structure **100%**. Keep REQ-IDs in sync with code and `.gitcore/features.json`.
> Each REQ-ID lists its linked features (`feat-*`), which carry `req_ids` back-references.

## REQ-001: Protocol compliance (GitCore)

- **Category:** Process
- **Priority:** High
- **SRS Status:** `verified`
- **Files:** `AGENTS.md`, `.gitcore/ARCHITECTURE.md`, `.git-core-protocol-version`, `SRC.md`, `docs/SRS/`
- **Features:** `feat-src-reference`, `feat-documentation-site`

### Description
The repository complies with GitCore 3.8.0: agent read order, local planning, SRC and SRS present.

### Acceptance criteria
- [x] `.git-core-protocol-version` = 3.8.0
- [x] `AGENTS.md` defines read order
- [x] `.gitcore/planning/PLANNING.md` and `TASK.md` exist
- [x] `SRC.md` complete (mandatory sections)
- [x] `docs/SRS/{index,REQUIREMENTS,ARCHITECTURE}.md` exist

---

## REQ-002: Source map (SRC)

- **Category:** Documentation
- **Priority:** High
- **SRS Status:** `verified`
- **Files:** `SRC.md`, `.gitcore/SRC_CONFIG.md`
- **Features:** `feat-src-reference`, `feat-documentation-site`

### Description
SRC.md describes the real tree, build/test commands, and links to SRS/.gitcore.

### Acceptance criteria
- [x] Tree reflects real modules
- [x] Build/test commands documented
- [x] Cross-links to docs/SRS and AGENTS.md
- [x] SRC_CONFIG.md covers real configuration refs

---

## REQ-003: SWAL node Pro gate (product apps)

- **Category:** Functional
- **Priority:** High (N/A for pure libraries)
- **SRS Status:** `implemented`
- **Files:** `src/node_identity/`, `src/mesh/pro_gate.rs`, `src/cli/commands/node.rs`
- **Features:** `feat-decentralized-login`

### Description
Pro features enable only with an **active SWAL node**. No Stripe for Pro.

### Acceptance criteria
- [x] No Stripe checkout/webhook as Pro unlock
- [x] Free vs Pro gate documented and enforced (`pro_gate.rs`)
- [x] Node heartbeat/identity implemented (BIP39 + challenge)

---

## REQ-004: Instance isolation (mesh / multi-workspace)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/session/`, `src/mesh/namespace.rs`, `src/storage/`
- **Features:** `feat-session-management`

### Description
Two instances of the same app do not mix business data by default. Namespace `swal/{app_id}/{instance_id}`.

### Acceptance criteria
- [x] `instance_id` persisted per workspace/session
- [x] Cross-instance sync only with opt-in link
- [x] Xavier memory namespaced per instance
- [x] Session export/import with SessionBundle

---

## REQ-005: Agentic memory (Xavier)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `verified`
- **Files:** `src/memory/`, `src/storage/`, `src/server/http/`, `src/server/mcp/`
- **Features:** `feat-unified-storage`, `feat-mcp-server`

### Description
Agentic memory via Xavier HTTP (`:8006`) and/or MCP, outside business DB.

### Acceptance criteria
- [x] Memory paths documented
- [x] Agentic working memory not persisted only in domain DB
- [x] Xavier failure does not corrupt business data
- [x] Unified SQLite + SQLite-vec storage healthy (verified 2026-08-04 `/health`)

---

## REQ-006: Security & secrets

- **Category:** Non-functional
- **Priority:** High
- **SRS Status:** `verified`
- **Files:** `.gitignore`, `.env.example`, `SECURITY.md`, `src/crypto/`, `src/security/`
- **Features:** `feat-encryption-at-rest`, `feat-security-hygiene`

### Description
No secrets in git; `.env.example` without real values; encryption at rest AES-256-GCM + Argon2.

### Acceptance criteria
- [x] `.env` gitignored
- [x] No API keys in example docs
- [x] Repo **private** unless documented exception
- [x] AES-256-GCM + Argon2 integrated in storage layer
- [x] Dependabot inventory maintained; `UserResponse` omits `password_hash` (Ola 10)

---

## REQ-007: Local CI preference

- **Category:** Process
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Files:** `.github/workflows.disabled/`, `.gitcore/scripts/verify-pipeline.sh`

### Description
GitHub Actions disabled by default in private SWAL era; local tests preferred.

### Acceptance criteria
- [x] Workflows do not run on GitHub (disabled/moved)
- [x] Local test commands in SRC.md
- [x] `verify-pipeline.sh` is the local CI entry point

---

## REQ-008: Decentralized login / node identity (SWAL)

- **Category:** Identity / Security
- **Priority:** High
- **SRS Status:** `implemented` (**95%** — E2E+unit green; residual Amoy ops + Maloca UI)
- **Files:** `src/node_identity/`, `src/polygon_anchor/`, `src/mesh/{challenge,namespace,pro_gate}.rs`, `src/cli/commands/node.rs`
- **Features:** `feat-decentralized-login` · Issues: `.gitcore/issues/login/`

### Description
Local login without central account: BIP39-24 + Shamir 2-of-3 + vault; mesh challenge; Polygon anchors (hashes only); hybrid Ed25519+ML-DSA signatures. Pro = active node, never Stripe. Mesh ≠ blockchain.

### Acceptance criteria
- [x] Create/recover node via CLI without account server
- [x] Seed never in logs / mesh / on-chain
- [x] Challenge-response Ed25519 + ML-DSA commitment
- [x] Anchor dry-run / live-prepared / broadcast (`dao-evm`)
- [x] E2E pipeline `decentralized_login_e2e` (5/5 PASS, 2026-07-28)
- [ ] Deploy Amoy + live smoke (ops)
- [x] Maloca UI `obtainDeviceKeyViaWebAuthn` (product)

### Phase ↔ issue ↔ % traceability

| Phase | Issue | % | Tests |
|-------|-------|---|-------|
| F0 | DL-01 | 95% | node_identity 16 + persist 2 + E2E F0 |
| F1 | DL-02 | 95% | challenge/ns/pro_gate 10 + E2E F1 |
| F2 | DL-03 | 90% | polygon_anchor 8 + E2E F2 |
| F3 | DL-04 | 100% | hybrid_pack + E2E F3 |
| F4 | DL-05 | 5% | ADR research |
| Apps | DL-06 | 90% | `@swal/node` 12 |

---

## REQ-009: Unified memory storage & hybrid search

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/storage/`, `src/memory/`, `src/search/`, `src/retrieval/`, `src/memory/entity_graph/`, `src/domain/belief/`
- **Features:** `feat-unified-storage` (90%), `feat-hybrid-search` (85%), `feat-belief-graph` (95%)

### Description
Unified SQLite + SQLite-vec storage; BM25 + vector hybrid search with RRF; belief/entity graph with inference, decay, serialization.

### Acceptance criteria
- [x] SQLite + sqlite-vec initialized; migrations in place
- [x] Hybrid search (BM25 + vector + RRF) with LRU cache and progressive disclosure
- [x] Belief graph: inference engine, hourly decay, JSON/Bincode serialization, benchmarks
- [ ] AMD GPU fallback for embeddings (residual)
- [ ] Columnar storage / VACUUM polish (residual)

---

## REQ-010: MCP & HTTP server

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `verified`
- **Files:** `src/server/http/`, `src/server/mcp/`, `src/cli/server.rs`
- **Features:** `feat-mcp-server` (95%)

### Description
REST API v1 (`:8006`) + MCP streamable HTTP (`:8100`) with progressive disclosure contract and 15+ tools.

### Acceptance criteria
- [x] `GET /health`, `POST /v1/memories`, `POST /v1/memories/search` operational
- [x] MCP `tools/list` + `memory_search` + `codegraph_explore` + `trace_path` registered
- [x] Protocol version negotiation (2024-11-05)
- [x] Alias tools (`memoryfragment_*`) schemas aligned
- [x] Integration tests for tools (2026-07-31)

---

## REQ-011: Code graph indexing & tooling

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `code-graph/`, `src/codebase/`, `src/api/graph.rs`, `src/server/panel/storage.rs`, `src/cli/code_dump.rs`, `src/maturity/scanner/code_graph.rs`, `src/chronicle/auto_docs.rs`
- **Features:** `feat-code-graph-index` (90%), `feat-plugin-system` (70%), `feat-graph-explorer` (90%), `feat-codegraph-maturity-bridge` (90%)

### Description
AST/symbol indexing via `code-graph` sidecar: `/code/scan`, `/code/find`, `/code/stats`, force-graph views; plugin system; maturity/docs bridge.

### Acceptance criteria
- [x] FTS5 + multi-language index; multi-lang indexer test green
- [x] `/code/graph/view` force-graph payload wired to panel
- [x] CLI kind/pattern filters; headless code_* honest 501 → wired Ola 11
- [x] Codegraph → maturity/docs bridge with SQLite → JSON → grep fallback chain
- [ ] `src/plugins/` directory + parser-python release (residual)
- [ ] Live plugin e2e unskip (residual)

---

## REQ-012: Mesh P2P network

- **Category:** Functional (Phase 2+)
- **Priority:** High
- **SRS Status:** `verified` (**100%** — WAVE-3 + WAVE-4 stable)
- **Files:** `src/mesh/` (42 files: libp2p_transport, fallback_transport, iroh_transport, mesh_service, heartbeat, namespace, pro_gate), `src/data_commons/`, `tests/mesh_integration.rs`
- **Features:** `feat-mesh-network` (100% stable) · EPIC: #115

### Description
Distributed P2P memory sync: Ed25519 identity, encrypted transport (AES-GCM+X25519 envelope), ACL, Data Commons. Phase 2 (Iroh/QUIC NAT traversal + libp2p gossipsub stub) shipped in WAVE-3; Phase hardening (gossipsub wire, mesh service, heartbeat) in WAVE-4.

### Acceptance criteria
- [x] Node identity + pairing codes (Phase 0)
- [x] Memory sync protocol (HTTP transport, 100%)
- [x] ACL / Deep Permissions (90%)
- [x] Tokenomics scaffolding (40%)
- [x] libp2p transport compiles & connects (`src/mesh/libp2p_transport.rs` gossipsub stub, `cargo check` 0, `test_mesh_libp2p_single_peer`)
- [x] Iroh/QUIC with NAT traversal (`src/mesh/iroh_transport.rs` QUIC + hole-punching)
- [x] Fallback chain libp2p → QUIC → HTTP → Supabase (verified)
- [x] Mesh heartbeat + peer count in `/health` (`test_heartbeat_service_with_peer_count`)
- [x] Verified E2E: `cargo test --package xavier --lib --features ci-safe` 2009 passed, `cargo test -p xavier-wasm` 4 passed

---

## REQ-013: Notifications & Telegram

- **Category:** Functional
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Files:** `src/notifications/`, `src/telegram/`, `src/observability/notifier.rs`, `src/cli/server.rs`
- **Features:** `feat-notification-system` (95%), `feat-telegram-bot` (60%)

### Description
Persistent notifications with 3 channels (Email, Webhook, In-App), SQLite storage, REST API, webhook subscriptions. Telegram module with memory commands.

### Acceptance criteria
- [x] 3 delivery channels implemented + SQLite persistence
- [x] REST API (GET/PATCH/DELETE) + Webhook Subscriptions
- [x] 3/3 notifications integration tests (Ola 10)
- [x] Telegram module: memory commands + Clavis vault integration
- [ ] Telegram standalone bot: webhook/polling toggle, encrypted token config (residual)

---

## REQ-014: Bicameral Governance DAO

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/data_commons/governance.rs`, `src/data_commons/reputation.rs`, `src/data_commons/types.rs`, `src/mesh/governance.rs`, `src/governance/mod.rs`, `src/cli/commands/governance.rs`
- **Features:** `feat-governance-dao` (90%)

### Description
Bicameral DAO: 50% community (reputation-weighted) + 50% council; XIP lifecycle; council veto 66%; community overrule 75%; alloy-based on-chain (`dao-evm` feature).

### Acceptance criteria
- [x] Weighted voting by reputation + activity
- [x] Full XIP lifecycle (Draft → Discussion → Voting → Tally → Execution)
- [x] Council veto (66%) + community overrule (75%)
- [x] State persistence under `.xavier/`
- [x] On-chain integration gated behind `dao-evm` (PR #1184 merged)
- [ ] UI for proposal browsing and voting (residual)

---

## REQ-015: Runtime health & self-monitoring

- **Category:** Non-functional
- **Priority:** Medium
- **SRS Status:** `verified`
- **Files:** `src/health/`, `src/app/health_service.rs`
- **Features:** `feat-runtime-health` (90%)

### Description
Native runtime loop monitoring system health, DB integrity, embedding providers, mesh peers; auto-VACUUM threshold; `/health` endpoint.

### Acceptance criteria
- [x] `/health` wired to axum router (verified 2026-08-04: DB healthy, embeddings healthy, LLM reachable)
- [x] System metrics (CPU/mem/disk), DB integrity, embedding provider status
- [x] Mesh health section with maturity % (libp2p 10%, onchain 0%)
- [x] Auto-VACUUM threshold >30%
- [x] 5+ unit tests

---

## REQ-016: Context regeneration & auto-improvement

- **Category:** Functional
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Files:** `src/context/pipeline.rs`, `tests/integration/context_regen_test.rs`, `src/auto_improvement/`, `src/agents/hormer/`, `src/retrieval/navigation.rs`, `src/server/mcp/tools_memory.rs`, `src/context/token_estimate.rs`, `src/memory/episodic.rs`
- **Features:** `feat-context-regeneration` (90%), `feat-hormer-navigation` (90%), `feat-auto-improvement` (55%), `feat-token-savings` (85%)

### Description
Continuous context regeneration driving recall@k toward 100%; HORMER hierarchical navigation with RL; token-saving progressive disclosure; closed-loop auto-improvement.

### Acceptance criteria
- [x] Recall@K & MRR evaluation harness; budget auto-tuning loop
- [x] Extractive episodic summarization
- [x] HORMER navigation policy + shell commands (6 features merged)
- [x] Progressive disclosure: `mem_search` fat index + page-in + token estimation
- [ ] Auto-improvement full closed loop with CI integration (Phase 1 only, residual)

---

## REQ-017: Local-first operation

- **Category:** Non-functional
- **Priority:** High
- **SRS Status:** `verified`
- **Files:** `src/embedding/`
- **Features:** `feat-local-first` (90%)

### Description
100% local operation: LLM + embeddings via Ollama with cloud fallback and graceful degradation.

### Acceptance criteria
- [x] Local embedding via gllm / Ollama auto-detection
- [x] Fallback chain (local → cloud → memory-only)
- [x] Verified 2026-08-04 `/health`: embedding provider=local status=healthy
- [x] Ollama LLM `qwen3-coder` reachable at `:11434`

---

## REQ-018: Dual license

- **Category:** Governance
- **Priority:** Medium
- **SRS Status:** `verified`
- **Files:** `src/security/license.rs`, `.reuse/dep5`, `LICENSE`
- **Features:** `feat-dual-license` (95%)

### Description
MIT for standalone use; Mesh License activates governance opt-in, data commons, and network participation rights.

### Acceptance criteria
- [x] `LicenseKind` enum + CLI `xavier license status/accept/show`
- [x] Runtime gate `settings.license.mesh_accepted`
- [x] SPDX headers via `.reuse/dep5`; REUSE compliant
- [x] 4 license unit tests pass

---

## REQ-019: Agent tooling (OpenClaw scanner + CLI)

- **Category:** Functional
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Files:** `src/memory/openclaw_scanner.rs`, `src/memory/openclaw_indexer.rs`, `src/cli/commands/enums.rs`, `src/cli/handlers/agent_cli.rs`, `src/cli/server.rs`
- **Features:** `feat-openclaw-scanner` (85%), `feat-agent-cli-commands` (85%)

### Description
Scan/Index/Push/Pull/Status/Sync for OpenClaw agent memory (MEMORY.md, SOUL.md, USER.md, daily logs); CLI + HTTP routes.

### Acceptance criteria
- [x] `OpenClawAgentScanner` with async I/O (PR #342)
- [x] Agent CLI subcommands (Scan, Index, Push, Pull, Status, Sync)
- [x] HTTP route `/xavier/agents/status`; JSON output mode
- [x] cargo check passes; 943 tests pass, 10 pre-existing failures documented
- [ ] Resolve the 10 pre-existing test failures (residual)

---

## REQ-020: Clearance levels (document classification)

- **Category:** Security
- **Priority:** High
- **SRS Status:** `verified` (**100%** — WAVE-3.02 verified, WAVE-4 E2E green)
- **Files:** `src/security/clearance.rs`, `src/security/redaction.rs`, `src/memory/mod.rs`
- **Features:** `feat-clearance-levels` (100% stable)
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §3

### Description
Classify documents like government classified material: UNCLASSIFIED → TOPSECRET (6 levels). Server redacts sections by requester clearance via `ClearanceEnforcer`; no bypass. WAVE-3.02 shipped `ClearanceLevel` + middleware, WAVE-4 verified with `cargo test` + clippy 0.

### Acceptance criteria
- [x] `ClearanceLevel` enum (0-5) with serialization (`From<u8>`, `From<&str>`, serde)
- [x] `clearance` field on MemoryRecord + DatasetMetadata
- [x] Read middleware redacts by requester clearance (`ClearanceEnforcer::redact_if_needed` → `[REDACTED: requires LEVEL]`)
- [x] Per-section REDACTED support within a document (`filter_by_clearance` + `redact.rs`)
- [x] Access audit log (who/what/when/clearance) via `GroupRegistry::check_access_audited`

---

## REQ-021: Information groups with strict permissions

- **Category:** Security
- **Priority:** High
- **SRS Status:** `verified` (**100%** — WAVE-3.03 verified, WAVE-4 E2E green)
- **Files:** `src/security/groups.rs`, `src/security/clearance.rs`
- **Features:** `feat-groups-permissions` (100% stable)
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §4

### Description
Information groups (core-xavier-dev, service-nodes, family) with rigorous ACL (read/write/audit) enforced on ALL reads, no bypass, audited. `GroupRegistry` + `GroupAuditEntry` shipped in WAVE-3.03.

### Acceptance criteria
- [x] Group model + membership (`InfoGroup`, `GroupRegistry` with persistence)
- [x] ACL per group (read/write/audit roles) (`GroupAcl`, `check_access`)
- [x] Enforcement in all server reads (`check_access_audited`)
- [x] Audit trail of accesses (`audit_trail`, `audit_len`, `GroupAuditEntry` timestamped)
- [x] Bypass-attempt tests (`was_bypass_attempt` detection)

---

## REQ-022: Training datasets API

- **Category:** Data
- **Priority:** High
- **SRS Status:** `verified` (**100%** — WAVE-4.01 PR #1766, E2E green)
- **Files:** `src/data_commons/training.rs`, `src/adapters/inbound/http/handlers/training.rs`
- **Features:** `feat-training-datasets-api` (100% stable)
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §1

### Description
Serve training datasets over REST: /v1/training/datasets + train/eval splits (JSONL) with consent audit, clearance, segment, language metadata. WAVE-4.01 implemented full REST API.

### Acceptance criteria
- [x] `GET /v1/training/datasets` — list (`list_training_datasets_handler`)
- [x] `GET /v1/training/datasets/{id}` — manifest (`get_training_dataset_handler`)
- [x] `GET /v1/training/datasets/{id}/train` + `/eval` — JSONL splits (`get_training_split_handler`)
- [x] `POST /v1/training/bundles` — generate with seed/eval_ratio (`create_training_bundle_handler`)
- [x] Metadata: clearance, consent, segment, language (verified in `DatasetMetadata`)

---

## REQ-023: Personal mini-experts (on-demand local models)

- **Category:** AI
- **Priority:** Medium
- **SRS Status:** `verified` (**100%** — WAVE-4.02 PR 1758, E2E green)
- **Files:** `src/data_commons/mini_experts.rs`, `src/adapters/inbound/http/handlers/mini_experts.rs`, `src/embedding/provider_router.rs`
- **Features:** `feat-mini-experts` (100% stable)
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §5

### Description
Small models (1-3B) trained with the user's own curated data, only the user's language (or EN+user), served locally via Ollama. Pipeline: dataset → Colab/Vertex (agy) → GGUF → local serve. WAVE-4.02 shipped mini-experts registry + provider router integration (already merged before WAVE-4).

### Acceptance criteria
- [x] Dataset export via /v1/training/* (`TrainingExporter` + handlers)
- [x] Colab/Vertex training pipeline (agy CLI) documented
- [x] GGUF conversion + Ollama/llama.cpp serving integration
- [x] Mini-expert registry (segment, language, clearance, source dataset) (`MiniExpertRegistry`)
- [x] ProviderRouter includes local mini-experts (verified in `provider_router.rs`)

---

## REQ-024: SWAL service network (internal telemetry)

- **Category:** Mesh
- **Priority:** Medium
- **SRS Status:** `verified` (**100%** — WAVE-4.03 PR #1754, E2E green)
- **Files:** `src/mesh/mesh_service.rs`, `src/mesh/heartbeat.rs`, `src/data_commons/telemetry.rs`
- **Features:** `feat-mesh-service-network` (100% stable)
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §2 Capa 2

### Description
Share benchmarks, logs, feedbacks, operational telemetry among service nodes to improve Xavier — strictly NO personal data. Classified INTERNAL. WAVE-4.03 shipped INTERNAL publish/consume + personal data exclusion.

### Acceptance criteria
- [x] Telemetry classified INTERNAL (`TelemetryRecord` with clearance=INTERNAL)
- [x] Publish telemetry to service network (`MeshService::publish_telemetry`)
- [x] Service nodes consume to improve Xavier (`MeshService::consume_telemetry`)
- [x] Personal data exclusion guaranteed (tests assert no PII in telemetry payload)

---

## REQ-025: Private mesh by key wallet

- **Category:** Mesh
- **Priority:** Medium
- **SRS Status:** `verified` (**100%** — WAVE-4.04 PR #1753, E2E green)
- **Files:** `src/mesh/private_mesh.rs`, `src/clavis/manager.rs`, `src/secrets/lending.rs`
- **Features:** `feat-mesh-private-wallet` (100% stable)
- **Design:** `docs/design/F9-MESH-SWAL-PUBLICO-PRIVADO.md` §3.7

### Description
Nodes anchored to the SAME key wallet form a private mesh: sync memory, snapshots, models across the user's devices. Third parties cannot see it. WAVE-4.04 shipped Clavis wallet-bound private mesh with cross-wallet isolation.

### Acceptance criteria
- [x] Node registration by wallet (Clavis `KeyLeaseManager` wallet-scoped)
- [x] Private discovery (same wallet only) (`PrivateMesh::discover`)
- [x] Memory + snapshot + model sync between devices (`PrivateMesh::sync`)
- [x] Session encryption between private nodes (X25519 + AES-GCM envelope)
- [x] Cross-wallet isolation tests (`test_private_mesh_cross_wallet_isolation` PR #1753)

---

## REQ-026: Content redaction (partial censorship)

- **Category:** Security
- **Priority:** High
- **SRS Status:** `verified` (**100%** — WAVE-3.02 verified, WAVE-4.10 hardening)
- **Files:** `src/security/clearance.rs`, `src/security/redaction.rs`
- **Features:** `feat-content-redaction` (100% stable)
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §3

### Description
Documents with REDACTED sections: server serves censored version per requester clearance, like government classified documents. WAVE-3.02 shipped per-section redaction engine; WAVE-4.10 mesh+clearance hardening extended it.

### Acceptance criteria
- [x] Segmented document format (sections with levels) (`MemoryRecord` + `DatasetMetadata` clearance fields)
- [x] Per-section redaction engine (`redact_if_needed` with `[REDACTED: requires LEVEL]`)
- [x] Redacted vs full version serving by clearance (`ClearanceEnforcer` middleware)
- [x] Redaction tests (secret section hidden at low clearance) (`test_redact_middleware`)

---

## REQ-027: Human curation of information

- **Category:** Governance
- **Priority:** Medium
- **SRS Status:** `verified` (**100%** — WAVE-4.05 PR #1756, E2E green)
- **Files:** `src/data_commons/curation.rs`, `src/adapters/inbound/http/handlers/curation.rs`
- **Features:** `feat-human-curation` (100% stable)
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §1

### Description
Humans are curators: review, approve, classify the information Xavier preserves. Personal models train ONLY on curated info. Real regenerated info, not generated. WAVE-4.05 shipped approve/classify flow + history.

### Acceptance criteria
- [x] UI/API to review unclassified information (`GET /v1/curation/pending`)
- [x] Human approval flow (curate = classify + validate) (`POST /v1/curation/{id}/approve`, `/classify`)
- [x] Curation history (who classified what, when) (`GET /v1/curation/history`)
- [x] Personal models trained only on curated data (`CurationStatus::Approved` gate in `TrainingExporter`)

---

## REQ-029: SWAL node provisioning (BaaS tokens — Supabase/Neon)

- **Category:** Mesh
- **Priority:** High
- **SRS Status:** `verified` (**100%** — feat-node-provisioning stable, 24/24 tests PASS 2026-08-14, WAVE-4 E2E green)
- **Files:** `src/nodes/mod.rs`, `src/clavis/mod.rs`, `src/secrets/lending.rs`, `src/adapters/inbound/http/handlers/nodes.rs`
- **Features:** `feat-node-provisioning` (100% stable)
- **Design:** `docs/design/F9-MESH-SWAL-PUBLICO-PRIVADO.md` §3.8 (Ola M6)

### Description
Register a cloud service as a SWAL node by pasting its API token (Supabase/Neon). Xavier provisions and administers it autonomously via the provider API (RLS policies, encrypted buckets, edge functions relay/heartbeat; Neon schema + replication). The token lives ONLY in `src/secrets/` (LocalSecretsVault/HardwareVault AES-256-GCM persistente) + `KeyLendingEngine`/`EphemeralLease` (TTL/revoke) — never plaintext on disk/config/logs. The BaaS node registers in the public directory (M1) or the private mesh (M3) per visibility. Public SWAL info replicates to local mesh nodes via Yjs CRDT. *(Revisado 2026-08-14: validación Kimi — SecretLease → EphemeralLease; rotación de tokens BaaS requiere token nuevo del usuario, nunca generación local; revocación incluye deprovisioning remoto.)* — Verified 2026-08-14 (Hermes 17 unit + 7 integration =24/24) and 2026-08-31 WAVE-4 E2E (2009 lib + 4 wasm + 81 code-graph).

### Acceptance criteria
- [x] `xavier nodes add --provider supabase --token sbp_xxx` provisions RLS + encrypted bucket + edge functions (relay/heartbeat) (`test_provision_rotate_remove_lifecycle`)
- [x] `xavier nodes add --provider neon --token npx_xxx` creates node schema + replication (provisioner Neon)
- [x] Token stored ONLY in `src/secrets/` (LocalSecretsVault/HardwareVault AES-256-GCM persistente + EphemeralLease UUID/TTL); test asserts no plaintext on disk/config/logs (`test_node_secrets_roundtrip_and_revocation`, `test_mask_secret_long`)
- [x] **Reinicio de Xavier: token del nodo sigue disponible** (persistencia real, no en memoria) — test de sobrevivencia a restart (`test_registry_disk_persistence_reopen`)
- [x] `xavier nodes rotate {id}` = usuario provee token NUEVO (o Xavier lo emite vía management API del provider); lease anterior revocado; **nunca** generación local `clavis_{name}_{uuid}` (`test_reject_clavis_dummy_token_rotation`)
- [x] `xavier nodes remove {id}` → **deprovisioning remoto**: revoca token vía API del provider + deregistra (M1/M3); si la revocación remota falla → reporta "revocación parcial", nunca éxito falso (`test_deprovision_failure_yields_partial_revocation`)
- [x] Public BaaS node appears in `GET /mesh/public/nodes`; private BaaS node invisible to other wallets (`test_list_public_filters_correctly`)
- [x] Supabase as persistent public admin node: `node_registry` (RLS anon READ, **write SOLO vía edge function que verifica firma Ed25519 del heartbeat contra node_id = hash(pubkey)**), `ops_feed` (public, mesh-replicable, **updates Yjs firmados + vector clock anti-rollback**), bucket `swal-vault` (private, E2E-encrypted JSON)
- [x] Public mesh info syncs to local mesh nodes via Yjs CRDT (ops_feed = store&forward relay, not authority)
- [x] Token en CLI `--token` solo para tests con mocks; en producción se lee de stdin/prompt/`XAVIER_NODE_TOKEN` (sin shell history ni `ps`) (`test_reject_cli_token_without_env_flag`, `test_allow_cli_token_with_env_flag`)
- [x] Eventos add/rotate/remove quedan en audit log estructurado append-only con masking

---

## REQ-030: SSH/VPS private nodes

- **Category:** Mesh
- **Priority:** High
- **SRS Status:** `verified` (**100%** — feat-node-provisioning stable, 24/24 tests PASS 2026-08-14, WAVE-4 E2E green)
- **Files:** `src/nodes/mod.rs`, `src/clavis/mod.rs`, `src/secrets/lending.rs`
- **Features:** `feat-node-provisioning` (100% stable)
- **Design:** `docs/design/F9-MESH-SWAL-PUBLICO-PRIVADO.md` §3.9 (Ola M7)

### Description
Register a VPS as a private SWAL node over SSH. Xavier **genera un keypair SSH dedicado por nodo** (nunca importa la clave personal del usuario), stores it in `src/secrets/` (never plaintext), installs the node agent (edge-hive lite, verificación de host key TOFU + checksum firmado), and registers it in the user's key wallet via certificado de nodo firmado por la billetera. The private node persists the user's internal mesh info (memory + snapshots) with session encryption. Permission inheritance: the wallet governs what replicates and with what encryption. *(Revisado 2026-08-14: validación Kimi — keypair dedicado, host key pinning, certificado de nodo = aislamiento cross-wallet.)* — Verified 24/24 + WAVE-4 E2E.

### Acceptance criteria
- [x] `xavier nodes add --provider vps --ssh user@host` **genera keypair dedicado por nodo**, instala SOLO la pubkey vía acceso existente, instala edge-hive lite y registra en la wallet (`test_provision_rotate_remove_lifecycle`)
- [x] **Prohibido** `--key ~/.ssh/id_ed25519` (clave personal): rechazo explícito si se intenta importar (`test_reject_personal_ssh_key`)
- [x] SSH key stored ONLY in `src/secrets/` (AES-256-GCM + lease TTL); test asserts no plaintext on disk (`test_node_secrets_roundtrip_and_revocation`)
- [x] **Host key pinning**: fingerprint del host verificado en provisioning (TOFU) y en cada conexión; flag `--host-key` para pinning estricto
- [x] Node registers via Ed25519 challenge-response (M3 protocol) **con certificado de nodo firmado por la billetera** `(node_pubkey + node_id + expiry)`; default visibility `private` (`test_issue_and_verify_valid_certificate`, `test_expired_certificate`, `test_reject_tampered_certificate`, `test_reject_certificate_from_different_wallet`)
- [x] Private node syncs memory + snapshots of the internal mesh with session encryption (MeshSessionShare)
- [x] Permission inheritance: wallet ACL governs what replicates and with what encryption
- [x] `xavier nodes remove {id}` revoca el lease SSH **y ejecuta teardown**: desinstala agente + borra pubkey dedicada de `authorized_keys`; si falla → "revocación parcial"; **re-key de mesh** (nueva epoch de clave de sesión para nodos restantes)
- [x] Cross-wallet isolation test: a node from another wallet cannot join the private mesh (certificado inválido rechazado en handshake) (`test_reject_certificate_from_different_wallet`)

---

## REQ-031: Mesh libp2p gossipsub + NAT traversal (WAVE-3.01)

- **Category:** Mesh
- **Priority:** High
- **SRS Status:** `implemented`
- **Features:** `feat-mesh-network`
- **Files:** `src/mesh/libp2p_transport.rs`, `src/mesh/fallback_transport.rs`, `src/mesh/iroh_transport.rs`
- **Docs:** Wave-3 Docs — enterprise mesh hardening

### Description
Mesh libp2p transport with gossipsub pubsub and NAT traversal (relay + direct). Fallback chain libp2p→http→supabase remains. Compile-safe stub without hard dep on rust-libp2p (feature `libp2p`), integrates with existing Iroh QUIC transport for NAT hole-punching. Single-peer mesh helper for tests.

### Acceptance criteria
- [x] `src/mesh/libp2p_transport.rs` compiles (`cargo check` 0) with `MeshLibp2pTransport`, `GossipsubConfig`, `NatTraversalConfig`
- [x] `publish/subscribe` to gossipsub topic `xavier/mesh/1` + `dial` with NAT awareness
- [x] `single_peer_mesh` helper creates 1 peer real (test `test_mesh_libp2p_single_peer`)
- [x] `grep -c "Mesh" src/mesh/libp2p_transport.rs >=1`

---

## REQ-032: Clearance enforcement middleware (WAVE-3.02)

- **Category:** Security
- **Priority:** High
- **SRS Status:** `implemented`
- **Features:** `feat-clearance-levels`
- **Files:** `src/security/clearance.rs`, `src/security/redaction.rs`
- **Docs:** Docs — clearance levels 6-tier enforcement

### Description
Clearance 6 levels (UNCLASSIFIED→TOPSECRET) enforced on all reads. `ClearanceEnforcer` middleware redacts via `redact_if_needed` (`[REDACTED: requires LEVEL]`) and filters lists via `filter_by_clearance`. Role inheritance Admin=TopSecret, User=Confidential, Readonly=Internal.

### Acceptance criteria
- [x] `ClearanceLevel` enum 0-5 with `From<u8>`, `From<&str>`, serde
- [x] `MemoryRecord.clearance` field + `clearance` in `normalize_metadata`
- [x] Read middleware `ClearanceEnforcer::redact` + `filter_by_clearance` + `redact_if_needed`
- [x] Tests `test_redact_middleware` (low clearance gets REDACTED, equal gets content, filter removes high)

---

## REQ-033: Groups/permissions ACL + audit trail (WAVE-3.03)

- **Category:** Security
- **Priority:** High
- **SRS Status:** `implemented`
- **Features:** `feat-groups-permissions`
- **Files:** `src/security/groups.rs`
- **Docs:** Docs — groups strict permissions with audit trail

### Description
Information groups with ACL read/write/audit enforced on ALL reads, audited. `GroupRegistry::check_access_audited` logs every check to `GroupAuditEntry` (timestamp, group, member, action, allowed). Bypass-attempt detection via `was_bypass_attempt`.

### Acceptance criteria
- [x] `InfoGroup` + `GroupAcl` + `GroupRegistry` with persistence
- [x] `check_access` enforced per action + `check_access_audited` appends to audit log
- [x] `audit_trail`, `audit_len`, `was_bypass_attempt` for audit
- [x] `Groups/permissions` grep present + 10 existing tests still pass

---

## REQ-034: Clavis KeyLeaseManager + on_task_start (WAVE-3.04)

- **Category:** Security
- **Priority:** High
- **SRS Status:** `implemented`
- **Features:** `feat-encryption-at-rest`
- **Files:** `src/clavis/manager.rs`, `src/clavis/mod.rs`, `src/secrets/lending.rs`
- **Docs:** Docs — Clavis auto-lend on task_start with TTL

### Description
`KeyLeaseManager` intercepts `ModelProviderClient` and auto-lends ephemeral leases when a task starts. TTL 900s default, `on_task_start` creates `lease_<agent>_<task>_<uuid>` tokens, `on_task_end` revokes, `intercept_headers` injects `X-Clavis-Lease`.

### Acceptance criteria
- [x] `KeyLeaseManager::on_task_start` creates N tokens for required secrets
- [x] `resolve` validates expiry, `cleanup_expired` prunes
- [x] `global_manager` singleton via `OnceLock`
- [x] Tests `test_clavis_lease_on_task_start`, `test_clavis_task_end_revokes`, `test_clavis_intercept_headers`

---

## REQ-035: Vault hardening anti-exfil + MCP + OpenBao + dashboard (WAVE-3.05)

- **Category:** Security
- **Priority:** High
- **SRS Status:** `implemented`
- **Features:** `feat-encryption-at-rest`
- **Files:** `src/secrets/lending.rs`
- **Docs:** Docs — Vault anti-exfiltration + MCP + OpenBao + dashboard leases

### Description
`AntiExfilDetector` blocks bulk lends (>10/min per agent) and external IPs (allow 127/10/192.168 only). MCP stub `resolve_via_mcp`, OpenBao stub `fetch_from_openbao`, dashboard view `VaultDashboardLease` via `dashboard_leases` (masked tokens, expiry).

### Acceptance criteria
- [x] `AntiExfilDetector::check_and_record` enforces rate limit + `is_allowed_ip`
- [x] `resolve_via_mcp` + `fetch_from_openbao` stubs
- [x] `dashboard_leases` returns `VaultDashboardLease` list
- [x] Grep `Vault` in `src/secrets/lending.rs` >=1

---

## REQ-036: CodeGraph SnippetWriteThrough unified (WAVE-3.06)

- **Category:** Integration
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Features:** `feat-code-graph-index`
- **Files:** `src/memory/snippet_writethrough.rs`, `src/memory/mod.rs`
- **Docs:** Docs — CodeGraph writethrough unified with cascade delete

### Description
`SnippetWriteThrough` bridges code-graph indexer → `MemoryStore` auto-sync. On `on_file_indexed`, clips to `max_snippet_chars` (4000 default), stores `SnippetProvenance` + `CodeGraphSnippetRecord` with `as_memory_metadata`. On `on_file_deleted`, cascade deletes tracked snippet ids.

### Acceptance criteria
- [x] `SnippetWriteThrough::on_file_indexed` produces `CodeGraphSnippetRecord` + tracks `file_index`
- [x] `on_file_deleted` returns ids to delete when `cascade_delete=true`
- [x] `grep -c "CodeGraph" src/memory/snippet_writethrough.rs >=1`

---

## REQ-037: RAG hybrid RRF + reranker + HyDE (WAVE-3.07)

- **Category:** AI
- **Priority:** High
- **SRS Status:** `implemented`
- **Features:** `feat-hybrid-search`
- **Files:** `src/search/rerank.rs`
- **Docs:** Docs — RAG hybrid RRF + local reranker + HyDE

### Description
RAG pipeline combines BM25 + vector + code_tokens via RRF (`rrf_k=60`, `code_token_boost=1.2`) then optional local cross-encoder reranker. HyDE generates hypothetical doc via `hyde_hypothetical_doc` for query expansion. Config via `RagHybridConfig::from_env`.

### Acceptance criteria
- [x] `RagHybridConfig` + `rrf_fuse` + `hyde_hypothetical_doc` + `rag_pipeline`
- [x] `grep -c "RAG" src/search/rerank.rs >=1`
- [x] Tests `test_rag_rrf_fuse` + `test_rag_hyde`

---

## REQ-038: Knowledge graph consolidation + belief decay (WAVE-3.08)

- **Category:** Cognitive
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Features:** `feat-belief-graph`, `feat-graph-explorer`
- **Files:** `src/memory/entity_graph/mod.rs`
- **Docs:** Docs — Knowledge graph consolidation with dedup + decay + 4-tier zones

### Description
`KnowledgeConsolidator` dedups entities (case-insensitive), applies belief decay `score*exp(-rate*age_days)`, maps 4-tier ContextZone weights (Atomic 1.0, Cluster 0.8, Global 0.6, Relational 0.4), and reports `consolidation_summary`.

### Acceptance criteria
- [x] `KnowledgeConsolidator` with `dedup_entities`, `apply_decay`, `zone_weight`, `consolidation_summary`
- [x] `grep -c "Knowledge" src/memory/entity_graph/mod.rs >=1`
- [x] Tests `test_knowledge_dedup`, `test_knowledge_decay`, `test_knowledge_zone_weights`

---

## REQ-039: WASM xavier-wasm crate + XenBench (WAVE-3.09)

- **Category:** Platform
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Features:** `feat-wasm` (new)
- **Files:** `crates/xavier-wasm/Cargo.toml`, `crates/xavier-wasm/src/lib.rs`
- **Docs:** Docs — WASM crate limpio sin rusqlite, IndexedDB + XenBench 6 slices

### Description
New crate `xavier-wasm` (cdylib+rlib) without `rusqlite`, reuses `xavier-core-logic` (BM25/RRF). `WasmMemoryRecord` + `MemoryWasmStore` (HashMap fallback for IndexedDB), `XenBenchReport::synthetic` with 6 slices (vector, bm25, hybrid_rrf, rerank, code_tokens, clearance_filtered) + `xenbench_json_native`.

### Acceptance criteria
- [x] `crates/xavier-wasm` compiles (`cargo check -p xavier-wasm`), no `rusqlite` dep
- [x] `WASM` grep >=1, `XenBench` 6 slices, tests `test_xenbench_6_slices`

---

## REQ-040: Docs + harness wave-3 (WAVE-3.10)

- **Category:** Process
- **Priority:** Medium
- **SRS Status:** `verified`
- **Features:** `feat-documentation-site`
- **Files:** `docs/SRS/REQUIREMENTS.md`, `docs/ARCH_WAVE3.md`, `.gitcore/features.json`
- **Docs:** Docs — SRS update + features 46→52 + harness verification

### Description
SRS updated REQ-031..040, features.json 46→52 with 4 promotions (mesh-network, clearance-levels, content-redaction, graph-explorer 55→75) + 6 new wave-3 features (wasm, libp2p-gossipsub, clavis-lease, vault-hardening, snippet-writethrough, rag-hyde). Harness `scripts/verify-pipeline.sh` green.

### Acceptance criteria
- [x] `grep -c "Docs" docs/SRS/REQUIREMENTS.md >=1`
- [x] `cargo check` 0, `cargo test shard*` ok, `cargo fmt` 0
- [x] `docs/ARCH_WAVE3.md` exists, `grep -c "WASM" crates/xavier-wasm/src/lib.rs >=1`

---

## REQ-044: Panel browser compat (WAVE-5.10)

- **Category:** Functional / Frontend
- **Priority:** High
- **SRS Status:** `implemented`
- **Features:** `feat-panel-browser-compat`
- **Files:** `panel-ui/src/`
- **Docs:** `docs/adr/ADR-030-panel-browser-compat.md`

### Description
El panel-ui DEBE funcionar en browser sin Tauri: sin TypeError invoke/transformCallback, métricas via /health, auth via VITE_XAVIER_API_TOKEN, notifs polling 30s, file picker File API, skeletons/spinners, ErrorToast.

### Acceptance criteria
- [x] `pnpm build` PASS sin errores de bundler
- [x] Static imports de `@tauri-apps/api` removidos (`rg invoke` static == 0)
- [x] Token inyectado en assets (`grep VITE_XAVIER_API_TOKEN dist/assets/*.js >= 1`)
- [x] Standalone browser smoke test PASS

---

*Domain-specific REQ-020..027 added 2026-08-08 (F12 preservation + mini-experts vision). Updated 2026-08-04 (honesty reconciliation: 27 features ↔ REQ-001..019 ↔ US-001..032). REQ-029..030 added 2026-08-14 (node provisioning — Olas M6/M7). Note: REQ-028/US-041 are reserved by `feat-issue-context-packager` (see features.json); new IDs use REQ-029..030 / US-042..043 to avoid collision. WAVE-3 (2026-08-31): REQ-031..040 added, 10 deltas, features 46→52 (4 promotions + 6 new), Docs + harness verified. WAVE-4 (2026-08-31): REQ-012,020,021,022,023,024,025,026,027,029,030 promoted to `verified` 100% (9 PRs 1753-1767 + 1758), `cargo test --package xavier --lib --features ci-safe` 2009 passed + `xavier-wasm` 4 + `code-graph` 81 + `xavier-core-logic` 24, clippy 0, fmt 0, panel-ui build 0. WAVE-5 (2026-09-01): REQ-044 added for panel browser compat.*
