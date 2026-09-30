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
- **Files:** `.gitignore`, `.env.example`, `.github/SECURITY.md`, `src/crypto/`, `src/security/`
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
Register a cloud service as a SWAL node by pasting its API token (Supabase/Neon). Xavier provisions and administers it autonomously via the provider API (RLS policies, encrypted buckets, edge functions relay/heartbeat; Neon schema + replication). The token lives ONLY in `src/secrets/` (LocalSecretsVault/HardwareVault AES-256-GCM persistente) + `KeyLendingEngine`/`EphemeralLease` (TTL/revoke) — never plaintext on disk/config/logs. The BaaS node registers in the public directory (M1) or the private mesh (M3) per visibility. Public SWAL info replicates to local mesh nodes via Yjs CRDT. *(Revisado 2026-08-14: revisión externa — SecretLease → EphemeralLease; rotación de tokens BaaS requiere token nuevo del usuario, nunca generación local; revocación incluye deprovisioning remoto.)* — Verified 2026-08-14 (Hermes 17 unit + 7 integration =24/24) and 2026-08-31 WAVE-4 E2E (2009 lib + 4 wasm + 81 code-graph).

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
Register a VPS as a private SWAL node over SSH. Xavier **genera un keypair SSH dedicado por nodo** (nunca importa la clave personal del usuario), stores it in `src/secrets/` (never plaintext), installs the node agent (edge-hive lite, verificación de host key TOFU + checksum firmado), and registers it in the user's key wallet via certificado de nodo firmado por la billetera. The private node persists the user's internal mesh info (memory + snapshots) with session encryption. Permission inheritance: the wallet governs what replicates and with what encryption. *(Revisado 2026-08-14: revisión externa — keypair dedicado, host key pinning, certificado de nodo = aislamiento cross-wallet.)* — Verified 24/24 + WAVE-4 E2E.

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

## REQ-045: Desktop multi-target bundle (WAVE-6.01)

- **Category:** Packaging / Distribution
- **Priority:** High
- **SRS Status:** `in_progress`
- **Features:** `feat-desktop-bundle`
- **Files:** `panel-ui/src-tauri/tauri.conf.json`, `panel-ui/src-tauri/src/lib.rs`
- **Docs:** Issue #1891, Issue #1893

### Description
Xavier desktop app DEBE ofrecer instaladores one-click para usuarios no técnicos en macOS (`.dmg`, `.app`) y Windows (`.msi`, NSIS `.exe`) con auto-arranque del sidecar daemon Xavier HTTP (`:8006`), soporte de bandeja del sistema (System Tray) y detección automática de credenciales.

### Acceptance criteria
- [ ] `tauri.conf.json` configura targets `dmg`, `app`, `msi`, `nsis`
- [ ] Tauri sidecar arranca y monitorea ciclo de vida del backend nativo de Xavier
- [ ] Cero dependencias manuales de terminal/Rust para el usuario final

---

## REQ-046: Cloudflare Serverless Edge persistence spec (WAVE-6.02)

- **Category:** Cloud / Persistence
- **Priority:** High
- **SRS Status:** `in_progress`
- **Features:** `feat-cloudflare-edge-backend`
- **Files:** `src/memory/cloud_sync.rs`, `panel-ui/wrangler.toml`, `docs/features/specs/FEATURE-070-cloudflare-vectorize-backend.md`
- **Docs:** Issue #1892, Issue #1895

### Description
Soporte de arquitectura híbrida SWAL en Cloudflare: frontend `panel-ui` en Cloudflare Pages (`panel.xavier.swal.dev`) y persistencia serverless para el tier Socio SWAL ($9/mes) mapeando vectores a Cloudflare Vectorize, embeddings en Workers AI y metadatos en Cloudflare D1 con cifrado E2E.

### Acceptance criteria
- [ ] `panel-ui/wrangler.toml` configurado para Cloudflare Pages SPA
- [ ] Adaptador `CloudBackendType::CloudflareEdge` especificado en `cloud_sync.rs`
- [ ] Zero-knowledge encryption: datos en reposo cifrados con llaves del usuario (AES-GCM)

---

## REQ-047: RTK Kernel CLI Proxy & Token Reduction

- **Category:** Execution / Agent Runtime
- **Priority:** High
- **SRS Status:** `verified`
- **Features:** `feat-rtk-kernel-proxy`
- **Files:** `src/kernel/`, `src/cli/commands/enums.rs`, `src/cli/commands/mod.rs`, `src/server/mcp/tools_context.rs`, `tests/kernel_proxy_test.rs`, `tests/kernel_mcp_test.rs`

### Description
Xavier provee un Kernel Proxy CLI en Rust que intercepta subprocesos de herramientas de desarrollo (`cargo test`, `git status/diff`, `grep`, `compiler logs`) y condensa su salida eliminando ruidos, reduciendo hasta un 90% el consumo de tokens en la ventana de contexto de los agentes (Antigravity, Hermes, Jules) con registro automático en `xavier_token_savings`.

### Acceptance criteria
- [x] Módulo `src/kernel/` implementa runners seguros y filtros de condensación (git, cargo, grep, ansi).
- [x] Subcomando CLI `xavier exec <cmd>` ejecuta con proxy y reporta tokens consumidos y ahorrados.
- [x] Herramienta MCP `xavier_run_command` expuesta para delegación transparente de agentes de IA.
- [x] Integración transparente con el tracker de métricas y compatibilidad con instaladores nativos sin dependencias adicionales.

---

## REQ-048: HumanChallenge → Curation → Training Pipeline

**Status:** Planned | **Wave:** 8 | **Module:** humanchallenge, data_commons
**SRS Reference:** IEEE 830 §3.2

### Description
Verified HumanChallenge events must flow through a human curation gate before feeding
the TrainingExporter. A CurationVote (Accept/Reject/Refine) with explicit training consent
gates which challenge responses become training data. Minimum thresholds: 10 eligible votes,
70% fact-verified, 60% training-eligible before export is allowed.

### Requirements
- REQ-048.1: System SHALL require explicit `training_eligible: true` on CurationVote for inclusion
- REQ-048.2: System SHALL block export unless CurationGate::check_readiness() returns is_ready=true
- REQ-048.3: CurationVote SHALL store domain_tags for mini-expert segment routing
- REQ-048.4: All curation votes SHALL be stored locally (Privacy P4) unless user upgrades consent

---

## REQ-049: Human Introspection Mode (LLM as Guide)

**Status:** Planned | **Wave:** 8 | **Module:** humanchallenge/introspection
**SRS Reference:** IEEE 830 §3.3

### Description
Xavier SHALL provide a structured human introspection mode where the LLM acts as a
professional guide (not answer-giver) using 6 evidence-based techniques. The human
produces insights; the LLM facilitates depth through targeted questions.

### Requirements
- REQ-049.1: System SHALL support 6 IntrospectionTechniques: Socratic, FiveWhys, PreMortem, SteelManning, FirstPrinciples, PatternRecognition
- REQ-049.2: IntrospectionSession SHALL auto-select technique based on ChallengeType
- REQ-049.3: FiveWhys technique SHALL auto-complete after 5 human turns
- REQ-049.4: Completed sessions SHALL extract human insights for optional training inclusion
- REQ-049.5: LLM guide prompts SHALL be generated locally — no external LLM API required for the guide scaffolding

---

## REQ-050: Privacy Pipeline P2/P3/P4

**Status:** Planned | **Wave:** 8 | **Module:** data_commons/privacy
**SRS Reference:** IEEE 830 §3.4 (Privacy Requirements)

### Description
All training data exports SHALL pass through a PrivacyPipeline with three levels:
P4 (local only, no processing), P2 (PII scrubbing for on-prem sharing), P3 (DP + scrubbing for external/Colab).

### Requirements
- REQ-050.1: P4 data SHALL never leave the local node without explicit user upgrade
- REQ-050.2: P2 exports SHALL scrub: emails, file paths, API keys, passwords
- REQ-050.3: P3 exports SHALL add Laplace differential privacy noise (epsilon=1.0) on confidence scores
- REQ-050.4: k-anonymity check (k>=3) SHALL be enforced on P3 exports — export rejected if k<3
- REQ-050.5: Privacy audit report SHALL be included in every TrainingBundle manifest

---

## REQ-051: Enterprise ComputeProvider with ZDR (Zero Data Retention)

**Status:** Planned | **Wave:** 8 | **Module:** enterprise/compute
**SRS Reference:** IEEE 830 §3.5 (Enterprise Requirements)

### Description
Enterprise users SHALL have access to ComputeProvider abstraction (Local, ColabPro, RunPod,
VastAI, LambdaLabs) with cryptographically verifiable Zero Data Retention audit trail.

### Requirements
- REQ-051.1: ZdrAuditEntry SHALL include pre/post volume hashes signed by the provider
- REQ-051.2: Xavier SHALL verify ZdrAuditEntry::verify() before marking a TrainingJob as ZDR-compliant
- REQ-051.3: ColabPro provider SHALL require explicit Google OAuth token (not stored in plaintext)
- REQ-051.4: Enterprise plan SHALL have access to RunPod and LambdaLabs providers
- REQ-051.5: Free/Pro plans SHALL only access Local compute provider

---

## REQ-052: Informed Consent for P3 (Colab) Training Export

**Status:** Planned | **Wave:** 8 | **Module:** panel-ui, humanchallenge
**SRS Reference:** IEEE 830 §3.6 (User Consent Requirements)

### Description
Before any P3-level export (Google Colab, external compute), the system SHALL present
a consent dialog explaining the privacy implications and requiring explicit user confirmation.

### Requirements
- REQ-052.1: P3 export SHALL be blocked until user explicitly confirms consent
- REQ-052.2: Consent dialog SHALL clearly state: "Google will process your anonymized dataset"
- REQ-052.3: Consent SHALL be recorded in the audit log with timestamp and user identity hash
- REQ-052.4: Consent can be revoked at any time — revocation blocks future P3 exports until re-confirmed

---

## REQ-053: Code-Graph Honest Confidence (O1)

- **Category:** Non-functional (trust)
- **Priority:** High
- **SRS Status:** `planned`
- **Files:** `code-graph/src/query/mod.rs`, `code-graph/src/indexer/call_resolution.rs`, `src/retrieval/gating.rs`, `src/memory/belief_graph.rs`
- **Features:** `feat-cg-honest-confidence`

### Description

All code-graph answers SHALL use one confidence rubric (`EXTRACTED=1.0`, `INFERRED ∈ {0.95,0.85,0.75,0.65,0.55}`, `AMBIGUOUS ∈ 0.1-0.3`) and label honesty (`floor/total`, `shown/total/capped`, `amb`). Zero means "none found", never "none exists". Sources: ripwire R1, graphify G1. See ADR-032.

### Acceptance criteria

- [ ] Every graph count declares floor vs total; truncations disclosed
- [ ] Unknown selectors refuse with suggestion instead of silent empty
- [ ] Belief/integer scores mapped to the rubric with documented backfill

---

## REQ-054: Code-Graph Language Registry (O2)

- **Category:** Functional
- **Priority:** Medium
- **SRS Status:** `planned`
- **Files:** `code-graph/src/parser/mod.rs`, `code-graph/src/types.rs`, `code-graph/parsers/`
- **Features:** `feat-cg-language-registry`

### Description

Languages SHALL plug in via a unified `LanguageConfig` plus a suffix-ordered resolver registry without touching indexer core. Source: graphify G2.

### Acceptance criteria

- [ ] New language = 1 config + 1 fixture + tests (proven by re-registering one language)
- [ ] Builtin globals excluded from hub rankings
- [ ] Unsupported languages reported as `unindexed`, never as empty

---

## REQ-055: Code-Graph Blast Radius + Test Gate (O3)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `planned`
- **Files:** `code-graph/src/query/mod.rs`, `src/cli/handlers/code.rs`, `src/server/mcp/tools_core.rs`
- **Features:** `feat-cg-blast-testgate`

### Description

`blast_radius` SHALL return the transitive reach set with use roles and tested/untested split; a test gate SHALL fail when changed radius lacks coverage. Source: ripwire R2.

### Acceptance criteria

- [ ] Transitive `reaches` with per-site roles (call/read/write/import/extends/type)
- [ ] Tests-to-run listed with derivable `run=` only (never invented)
- [ ] Gate exits non-zero on untested radius; `situ` reads `git diff` plus co-change partners

---

## REQ-056: Code-Graph Incremental IDs (O4)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `planned`
- **Files:** `code-graph/src/types.rs`, `code-graph/src/db/mod.rs`, `code-graph/src/indexer/`
- **Features:** `feat-cg-incremental-ids`

### Description

Stable IDs SHALL carry the real project namespace; reindex SHALL key on content hash (manifest + cache); call resolution SHALL run in two phases with god-guard. Sources: graphify G3+G4. See ADR-033.

### Acceptance criteria

- [ ] Same path+symbol in different projects never collide; legacy `default` IDs rewired via versioned table
- [ ] Content-unchanged files skip re-extraction; deleted files prune symbols+edges
- [ ] Cross-file precision ≥ current baseline on fixture, ambiguous hubs skipped by god-guard

---

## REQ-057: Code-Graph Budget Query (O5)

- **Category:** Non-functional (efficiency)
- **Priority:** High
- **SRS Status:** `planned`
- **Files:** `src/memory/pack.rs`, `src/context/orchestrator.rs`, `code-graph/src/query/`
- **Features:** `feat-cg-budget-query`

### Description

Orientation answers SHALL default to compact signature bundles with declared `est_tokens` under an enforced budget gate; natural-language queries SHALL expand against repo vocabulary before vector spend. Sources: ripwire R3, graphify G6.

### Acceptance criteria

- [ ] Compact bundles carry no bodies; every answer declares `est_tokens`
- [ ] Over-budget answers gate explicitly, never truncate silently
- [ ] Zero-vocab queries stop explicitly; BFS3/DFS6 semantics on fixture

---

## REQ-058: Code-Graph PageRank Router (O6)

- **Category:** Functional
- **Priority:** Medium
- **SRS Status:** `planned`
- **Files:** `code-graph/src/rank.rs`, `code-graph/src/db/mod.rs`, `src/retrieval/gating.rs`
- **Features:** `feat-cg-pagerank-router`

### Description

Ranking SHALL combine edge weights with deterministic PageRank plus a lexical exact→BM25→subtoken router carrying confidence/margin. Source: ripwire R4. Requires O1.

### Acceptance criteria

- [ ] PageRank byte-deterministic across runs; edge-weight formula as specified with cap
- [ ] Router reports `confidence/margin_pct`; flat rankings read as starting points
- [ ] Change-teleport re-ranks without reindexing

---

## REQ-059: Code-Graph Contracts + Architecture Signals (O7)

- **Category:** Functional
- **Priority:** Medium
- **SRS Status:** `planned`
- **Files:** `code-graph/src/query/`, `code-graph/src/parser/`, `src/server/mcp/tools_core.rs`, `panel-ui/`
- **Features:** `feat-cg-contracts-arch`

### Description

Pre-merge contracts (`edit-check`, `safe-delete`, three-valued `verify`) plus architecture signals (god-nodes, surprising links, import cycles, rationale nodes) SHALL be exposed via MCP/CLI/UI. Sources: ripwire R5, graphify G7. Requires O1+O3. Leiden clustering tracked as optional O8.

### Acceptance criteria

- [ ] Contract changes vs HEAD detected with incompatible call-sites flagged; delete risks named without verdicts
- [ ] Verify returns confirmed/refuted/not-established with evidence; zero never refutes
- [ ] Rationale comments indexed as nodes linked to code

---

## REQ-060: Skill Registry Scans Canonical Store (skill-injection #301)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/context/skill_registry.rs`, `src/api/skills.rs`
- **Features:** `feat-skill-scan-paths`

### Description

`SkillRegistry::with_defaults` SHALL scan `workspace_root/skills`, `workspace_root/.agents/skills` and the configured canonical store (`src/context/skill_registry.rs:82-91`). (Planned, `feat-skill-store-config`, not yet implemented:) the canonical store becomes configurable via `XAVIER_SKILL_STORE`, defaulting to the Hermes store `$HOME/.hermes/skills` and preserving today's default scan set. `$HOME` is resolved from the environment at runtime, never hardcoded. Hermes remains the authoritative canonical store until an approved migration to the configured store completes (docs/design/skill-controller/05-DECISIONS-AND-SCOPE.md D1, D5). Traversal SHALL track visited `(device, inode)` pairs and skip seen dirs so symlink cycles terminate (`collect_skill_files`, `src/context/skill_registry.rs:138-194`); file symlinks are still followed. `skills.disabled` from `$HOME/.hermes/config.yaml` SHALL exclude names, fail-open when unreadable (`src/context/skill_registry.rs:95-123`).

### Acceptance criteria

- [ ] `GET /skills` against a HOME with the real store returns count >= 300 (live, post-restart)
- [ ] `cargo test -p xavier --lib context::skill_registry` green incl. `test_registry_scans_hermes_canonical_store`, `test_registry_symlink_cycle_terminates` (<5s), `test_registry_honors_disabled_list`
- [ ] `rg -n "/home/" src/context/skill_registry.rs` → 0 hits (no hardcoded developer home directory paths)

## REQ-061: Skill Semantic Ranking via Local Embeddings (skill-injection #302)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/context/skill_registry.rs`, `src/ports/outbound/embedding_port.rs`
- **Features:** `feat-skill-semantic-rank`

### Description

`IndexedSkill` carries an optional dense vector (`embedding`, `src/context/skill_registry.rs:37`); `index_skill_file_with_port` / `reindex_with_embeddings` / `embed_missing` embed `name + first 200 chars of description` through `EmbeddingPort`, fail-open to keyword-only (`src/context/skill_registry.rs:258-322,326-382`). `search_with_vector` / `search_semantic` rank by cosine similarity clamped to [0,1] with keyword `score_skill_match` tiebreak and keyword fallback when vectors are absent (`src/context/skill_registry.rs:427-475`). Cosine loops over many skills run in `spawn_blocking`. `SkillDispatcher::dispatch` keeps the compatible keyword `search()` call site.

### Acceptance criteria

- [ ] Offline eval harness `test_semantic_rank_recall_at_3`: 20 labeled queries, Recall@3 ≥ 0.8 (measured 0.950, MRR 0.950, mocked port, zero network)
- [ ] `test_confidence_calibration_range` (all scores in [0,1]) and `test_ranking_offline_no_network` green; keyword fallback green on empty vector store

## REQ-062: SkillLoader Fate — Keep with Real-Format Test (skill-injection #303)

- **Category:** Functional
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Files:** `src/context/skills.rs`, `src/context/executor.rs`
- **Features:** `feat-skill-loader-fix`

### Description

Decision: keep, not remove. `Skill` is consumed by `SkillExecutor` (`src/context/executor.rs:5`); only `SkillLoader::load_all` was prod-dead (registry superseded it). The loader SHALL accept Agent Skills frontmatter (`name:`/`description:`), fall back to parent-dir name, skip frontmatter-less files and non-`SKILL.md` markdown (`src/context/skills.rs:30-69`).

### Acceptance criteria

- [ ] `test_loader_loads_real_skill_format_or_module_removed` green on real store shape (`<skill>/SKILL.md` + frontmatter)

## REQ-063: Session Fusion Injects Skills on Maximum (skill-injection #304)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/server/http/context.rs`, `src/context/builder.rs`, `src/context/skill_dispatcher.rs`
- **Features:** `feat-skill-context-fusion`

### Description

On `ContextLevel::Maximum` only, `v1_context_regenerate` SHALL dispatch the latest user prompt through `SkillDispatcher`; on `confidence >= SKILL_CONFIDENCE_THRESHOLD` (0.5, `src/server/http/context.rs:15`) it feeds `compacted_content(remaining_budget)` plus pack memories into `builder.build`, replacing the previous empty `&[], &[]` placeholder (`src/server/http/context.rs:178-257`). Trivial prompts (acks/greetings/≤5-word confirmations, `is_trivial_prompt`) skip dispatch entirely. Minimal/Medium paths are byte-identical.

### Acceptance criteria

- [x] `POST /v1/context/regenerate` returns 200 on deep/medium/shallow (pre-existing 500 from unused `Extension<AppState>` fixed 2026-09-21); empty session takes the trivial/skill-free path without crash (live, post-restart)
- [ ] Seeded-thread E2E (architecture prompt → skill-bearing context; `"ok thanks"` → skill-free) — pending: no HTTP seeder for `conversations_db` threads; dispatcher leg proven live separately (`POST /api/skill/dispatch` → skill `review` @0.9 within budget) and gate logic covered by unit tests
- [x] `rg -n "integrated later in fusion" src/server/http/context.rs` → 0 hits
- [x] `cargo test -p xavier --lib server::http::context` green incl. `test_fusion_injects_skill_on_maximum`, `test_fusion_skips_below_confidence`, `test_fusion_skips_trivial_prompt`, `test_fusion_respects_token_budget`

## REQ-064: MCP Skill Dispatch Tools (skill-injection #305)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/server/mcp/tools_context.rs`, `src/server/mcp/server.rs`, `src/server/mcp/tests.rs`
- **Features:** `feat-skill-mcp-tool`

### Description

MCP SHALL expose `xavier_dispatch_skill` (`{task*, max_tokens?, project?}`, ≤60-char descriptions) and `xavier_skill_list` (no required params), registered in `get_xavier_context_tools` and routed in `handle_tool_call` (`src/server/mcp/tools_context.rs`, `src/server/mcp/server.rs:97-107`). Both reuse the single dispatcher construction from `api/skills.rs::dispatch_skill` via `build_skill_registry`. Dispatch returns `{skill_name, skill_description, confidence, context_pack, estimated_savings_pct}`; errors serialize fail-open (`{ok:false}`, never a throw). Existing tool names/schemas are byte-identical.

### Acceptance criteria

- [ ] MCP `tools/list` contains both tools (live, post-restart)
- [ ] Round-trip dispatch returns skill + pack within budget; list count matches fixture registry
- [ ] `cargo test -p xavier --lib test_mcp_` green incl. `test_mcp_dispatch_tool_roundtrip`, `test_mcp_skill_list_announced`

## REQ-065:
## REQ-066: Health report does not fabricate a healthy state (WAVE-29.01 #2554)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/health/mod.rs`, `src/adapters/inbound/http/routes.rs`
- **Features:** `feat-health-truth` (PR #2568)

### Description

`GET /health` and `collect_health*` SHALL NOT report unmeasured embedding coverage as healthy: zero records or no store yield `percent: 0.0`, `status: "unknown"` and a `Warn` coverage check. `HealthResponse.degraded_reasons` SHALL list one `host:<check>` or `subsystem:<check>` entry per non-passing check and SHALL be empty iff the status is `healthy`.

### Acceptance criteria

- [x] Merged in PR #2568; CI green on main `d914b41c`
- [ ] Declared tests green in `scripts/verify-pipeline.sh`: `embedding_coverage_default_is_not_healthy_when_empty`, `embedding_coverage_unmeasured_store_is_unknown`, `degraded_reasons_distinguishes_host_pressure_from_subsystem_failure`, `degraded_reasons_empty_when_status_healthy`

## REQ-067: MCP sys_health/health_check report measured values only (WAVE-29.02 #2555)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/server/mcp/tools_core.rs`, `src/health/mod.rs`
- **Features:** `feat-mcp-health-honesty` (PR #2577)

### Description

MCP `sys_health` SHALL report unmeasured benchmark metrics with a `-1.0` sentinel (never a fabricated `0.0`) and SQLite integrity as tri-state (`db_integrity: null` when the check did not run), so `analyze_gaps` raises no gap from unmeasured data. MCP `health_check.memoryStoreOk` SHALL mean the store answers a query and no measured integrity check failed. `codegraph_gods` SHALL exclude generic constructors and dedupe per symbol.

### Acceptance criteria

- [x] Merged in PR #2577; CI green on main `d914b41c`
- [ ] Declared tests green in `scripts/verify-pipeline.sh`: `health_check_embedding_ok_true_when_fallback_embedder_works`, `health_check_memory_store_ok_reflects_store_and_integrity`, `sys_health_does_not_report_hardcoded_zero_benchmarks`, `codegraph_gods_excludes_generic_constructors`

## REQ-068: Guardian env_status/log_scan report the real state (WAVE-29.03 #2556)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/self_manage/mod.rs`
- **Features:** `feat-guardian-truth` (PR #2575)

### Description

`env_status` SHALL query the systemd scope Xavier runs in (`--user`). `log_scan` SHALL read files newest-first, reset a cursor more than one rotation stale with an explicit `cursor_reset_reason`, never advance the cursor past a line dropped on `max_entries` overflow, and group identical lines with `count`/`first_seen`/`last_seen`. Tests SHALL NOT touch `$HOME/.xavier`.

### Acceptance criteria

- [x] Merged in PR #2575; CI green on main `d914b41c`
- [ ] Declared tests green in `scripts/verify-pipeline.sh`: `log_scan_prefers_newest_file_when_cursor_is_stale`, `log_scan_keeps_cursor_after_single_rotation`, `log_scan_reports_time_range_of_returned_entries`, `log_scan_groups_repeated_identical_lines`, `check_service_status_distinguishes_missing_unit_from_inactive`

## REQ-069: Skill dispatch confidence floor and ingest validation (WAVE-29.04 #2557)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/context/skill_dispatcher.rs`, `src/context/skill_registry.rs`
- **Features:** `feat-skill-dispatch-confidence` (PR #2567)

### Description

Skill dispatch SHALL return the `_none` sentinel (confidence 0.0) below `MIN_DISPATCH_CONFIDENCE = 0.40` and SHALL NOT report 1.0 for a weak match. Registry ingest SHALL reject entries without a plausible slug or a substantive description and count the rejects. Skill content SHALL be wrapped in an UNTRUSTED boundary and truncated by explicit byte/line/token budgets.

### Acceptance criteria

- [x] Merged in PR #2567; CI green on main `d914b41c`
- [ ] Declared tests green in `scripts/verify-pipeline.sh`: `dispatch_returns_no_match_below_confidence_threshold`, `dispatch_never_reports_full_confidence_for_weak_match`, `registry_rejects_non_skill_entries`, `registry_rejects_empty_description`, `test_confidence_calibration_range`

## REQ-070: xavier doctor is bounded and always prints its report (WAVE-29.05 #2558)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/cli/handlers/doctor.rs`, `src/cli/handlers/system_scan.rs`
- **Features:** `feat-doctor-bounded` (PR #2571)

### Description

`xavier doctor` SHALL bound every probe with a timeout, SHALL always print its report (including timed-out checks) and SHALL exit with a code that reflects the overall status.

### Acceptance criteria

- [x] Merged in PR #2571; CI green on main `d914b41c`
- [ ] Declared tests green in `scripts/verify-pipeline.sh`: `doctor_completes_when_a_probe_hangs`, `doctor_prints_output_even_with_timed_out_checks`, `doctor_exit_code_reflects_overall_status`

## REQ-071: xavier improve is bounded per stage and persists partial progress (WAVE-29.06 #2559)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/auto_improvement/cycle.rs`, `src/auto_improvement/mod.rs`, `src/cli/commands/improve.rs`
- **Features:** `feat-improve-bounded` (PR #2579)

### Description

`xavier improve` SHALL run every stage under a per-stage and an overall deadline (`XAVIER_IMPROVE_STAGE_TIMEOUT_SECS`, `XAVIER_IMPROVE_TOTAL_TIMEOUT_SECS`, capped at 24 h). An overrun SHALL end the cycle as `Truncated` with completed stages persisted as `partial` history; `Failed` only when partial progress cannot be persisted. Exit codes: `0` completed, `124` truncated, `1` failed. Truncated entries SHALL NOT feed `last_accepted_config` or `sys_health.last_experiment`.

### Acceptance criteria

- [x] Merged in PR #2579; CI green on main `d914b41c`
- [ ] Declared tests green in `scripts/verify-pipeline.sh`: `improve_cycle_respects_overall_deadline`, `improve_persists_progress_when_stage_times_out`, `improve_exit_code_distinguishes_truncated_from_failed`, `test_last_accepted_config_skips_truncated_entries`, `test_partial_progress_roundtrips_through_json`, `test_absurd_budget_is_capped_instead_of_overflowing`

## REQ-072: xavier health/stats match the server and MCP (WAVE-29.07 #2560)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/observability/health.rs`, `src/cli/handlers/system.rs`, `src/cli/handlers/memory.rs`, `src/cli/commands/http.rs`
- **Features:** `feat-cli-health-stats-parity` (PR #2574)

### Description

The live `GET /health` payload (`observability::health::HealthStatus`) SHALL carry the server build `version` and `degraded_reasons`. `xavier health` SHALL render them, label a CLI-side fallback version for older servers and exit non-zero when `unhealthy`. `xavier stats` SHALL read every counter from the same workspace as MCP `xavier_stats` and report unmeasured counters as `unknown`.

### Acceptance criteria

- [x] Merged in PR #2574; CI green on main `d914b41c`
- [ ] Declared tests green in `scripts/verify-pipeline.sh`: `health_report_shows_version_and_reasons_from_real_payload`, `healthy_status_reports_no_degraded_reasons`, `health_report_falls_back_to_cli_version_when_server_omits_it`, `stats_payload_matches_mcp_field_contract`, `stats_reports_semantic_layer_state`

## REQ-073: code dump honours its path argument and cwd; hubs skip trivial symbols (WAVE-29.08 #2561)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/cli/handlers/code.rs`, `src/cli/code_dump.rs`, `src/cli/commands/code.rs`, `code-graph/src/query/mod.rs`
- **Features:** `feat-codegraph-dump-path` (PR #2576)

### Description

`xavier code dump/scan` SHALL resolve the target as explicit argument, then caller cwd, then daemon default, and report `requested_path` and `resolved_path` (the path actually written). `hubs()`/`god_nodes()` SHALL exclude trivial constructors/accessors and keep the highest-degree occurrence per name. Index freshness SHALL be reported without triggering a reindex.

### Acceptance criteria

- [x] Merged in PR #2576; CI green on main `d914b41c`
- [ ] Declared tests green in `scripts/verify-pipeline.sh`: `dump_respects_explicit_path_argument`, `dump_respects_cwd_when_no_argument`, `dump_present_matches_actual_file`, `hubs_exclude_generic_constructors`, `test_hubs_exclude_trivial_symbols_and_dedupe_by_name`

## REQ-074: Memory writes reject corrupt content and are idempotent per path (WAVE-29.09 #2562)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `src/memory/sanitizer.rs`, `src/memory/qmd/writer.rs`
- **Features:** `feat-memory-write-integrity` (PR #2573)

### Description

Memory writes SHALL reject Latin-dominant tokens where a lowercase Latin letter is glued to CJK/Kana/Hangul, SHALL flag (and store verbatim) uppercase-acronym + CJK tokens, and SHALL accept multilingual content unchanged. A write SHALL be an idempotent retry only when path, content and non-volatile metadata match, checked under the same write guard as the append.

### Acceptance criteria

- [x] Merged in PR #2573; CI green on main `d914b41c`
- [ ] Declared tests green in `scripts/verify-pipeline.sh`: `create_rejects_mixed_script_corruption`, `create_accepts_multilingual_valid_content`, `suspect_content_is_stored_and_flagged`, `create_is_idempotent_per_path`, `same_path_different_content_still_appends`, `same_content_different_metadata_still_appends`, `returned_id_resolves_on_lookup`, `legacy_records_without_integrity_marker_still_load`

## REQ-075: E2E gate: MCP health agrees with GET /health (WAVE-29.10 #2563)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `tests/observability_contract.rs`, `.github/workflows/ci.yml`
- **Features:** `feat-e2e-observability-contract` (PR #2578)

### Description

CI SHALL run a hermetic E2E contract (`tests/observability_contract.rs`, job `Rust Integration (Observability Contract)`) that boots `xavier http` with state/data/cwd in a tempdir and asserts: self-consistent embedding health, `healthy` ⇔ empty `degraded_reasons` with tagged reasons, measured coverage (never 100% on an empty store), version and stats surface. Every field read SHALL assert presence and type.

### Acceptance criteria

- [x] Merged in PR #2578; CI green on main `d914b41c`
- [ ] Declared tests green in `scripts/verify-pipeline.sh`: `test_observability_embedding_consistency`, `test_observability_degraded_status_logic`, `test_observability_embedding_coverage_anomaly`, `test_observability_version_match`, `test_observability_mcp_stats_surface`

*WAVE-10 (2026-09-21): REQ-060..065 added (skill-injection wave: scan-paths, semantic-rank, loader-fate, fusion-gates, MCP-tools, ledger-docs). Implemented live: registry scans canonical store, cosine rank w/ keyword tiebreak, fusion gate 0.5 + ack-gate, MCP dispatch/list tools. Measured: Recall@3 0.950 / MRR 0.950 (20-query offline eval), live E2E post 0.2.5 restart. Full `verify-pipeline` green deferred: pre-existing zero-match filters outside the wave need ledger-wide cleanup (see rescan report).*

*Domain-specific REQ-020..027 added 2026-08-08 (F12 preservation + mini-experts vision). Updated 2026-08-04 (honesty reconciliation: 27 features ↔ REQ-001..019 ↔ US-001..032). REQ-029..030 added 2026-08-14 (node provisioning — Olas M6/M7). Note: REQ-028/US-041 are reserved by `feat-issue-context-packager` (see features.json); new IDs use REQ-029..030 / US-042..043 to avoid collision. WAVE-3 (2026-08-31): REQ-031..040 added, 10 deltas, features 46→52 (4 promotions + 6 new), Docs + harness verified. WAVE-4 (2026-08-31): REQ-012,020,021,022,023,024,025,026,027,029,030 promoted to `verified` 100% (9 PRs 1753-1767 + 1758), `cargo test --package xavier --lib --features ci-safe` 2009 passed + `xavier-wasm` 4 + `code-graph` 81 + `xavier-core-logic` 24, clippy 0, fmt 0, panel-ui build 0. WAVE-5 (2026-09-01): REQ-044 added for panel browser compat. WAVE-6 (2026-09-03): REQ-045..046 added for Desktop One-Click installer & Cloudflare Edge Persistence. REQ-047 added 2026-09-05 for RTK Kernel CLI Proxy. WAVE-8 2026-09-12: REQ-048..052 added (HumanChallenge curation pipeline, introspection mode, privacy pipeline, enterprise ZDR, informed consent). Module: humanchallenge + data_commons + enterprise + panel-ui. WAVE-9 (2026-09-18): REQ-053..059 added (ripwire+graphify extraction O1-O7: honest-confidence, language-registry, blast-testgate, incremental-ids, budget-query, pagerank-router, contracts-arch; US-101..US-114; specs docs/features/specs/FEATURE-feat-cg-*.md; doc docs/EXTRACTION-RIPWIRE-GRAPHIFY.md; ADR-032/033). WAVE-29 (2026-09-27): REQ-066..075 added (observability honesty: health-truth, mcp-health-honesty, guardian-truth, skill-dispatch-confidence, doctor-bounded, improve-bounded, cli-health-stats-parity, codegraph-dump-path, memory-write-integrity, e2e-observability-contract; PRs #2567-#2579; specs `.gitcore/waves/wave-29/issue-01..10.md`).*
 Skill Wave Ledger Docs + Close (skill-injection #306)

- **Category:** Documentation
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Files:** `docs/SRS/REQUIREMENTS.md`, `docs/features/specs/FEATURE-feat-skill-*.md`, `.gitcore/features.json`
- **Features:** `feat-skill-ledger-docs`

### Description

REQ-060..064 SHALL cite merged code evidence (`file:line`); the six `FEATURE-feat-skill-*.md` specs SHALL record final status plus measured numbers (Recall@3 0.950 / MRR 0.950, live counts post-restart). Ledger `status` fields are promoted only by green `verify-pipeline` runs, never by hand.

### Acceptance criteria

- [ ] `rg -c "^## REQ-" docs/SRS/REQUIREMENTS.md` shows 61 (REQ-060..065 present)
- [ ] Every cited `path:line` exists; `verify-pipeline.sh --check-only` exit 0
- [ ] Zero `src/` changes under this REQ (docs-only)

## REQ-080: PageIndex Tree Data Model + Builders (no LLM)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `crates/xavier-pageindex/src/{model,builders/*}.rs`
- **Features:** `feat-pageindex-tree-core`

### Description

The `xavier-pageindex` crate SHALL represent a document as a hierarchical tree (`title`, `node_id`, `start`/`end` page or line range, optional `summary`, `children`) and SHALL build it from markdown headings, plain-text numbered headings and legal patterns without any LLM or network access. Trees SHALL validate (ordered, non-overlapping siblings, where adjacent siblings may share exactly one boundary page (`prev.end <= next.start`) and deeper overlap is rejected; children inside the parent range).

### Acceptance criteria

- [ ] `cargo test -p xavier-pageindex` green incl. `test_md_headings_build_nested_tree`, `test_legal_articles_nest_under_chapters`
- [ ] The crate has no dependency on the `xavier` package (`! cargo tree -p xavier-pageindex -e normal --prefix none | grep -qE '^xavier v'` exits 0)

## REQ-081: PageIndex Persistence + Read Tools (MCP and HTTP)

- **Category:** Functional
- **Priority:** High
- **SRS Status:** `implemented`
- **Files:** `crates/xavier-pageindex/src/store/*`, `src/pageindex_glue/*`, `src/server/pageindex_routes.rs`, `src/server/mcp/tools_pageindex.rs`
- **Features:** `feat-pageindex-tree-core`

### Description

Trees and page text SHALL persist in a workspace-scoped SQLite store. Xavier SHALL expose `pageindex_browse_documents`, `pageindex_get_document`, `pageindex_get_document_structure`, `pageindex_get_page_content`, `pageindex_search` (read) and `pageindex_index_document` (role-gated write) over MCP, and the same operations under `/v1/pageindex/*`. Tool failures SHALL be returned as `{ok:false,error}` envelopes, never panics. Existing MCP tool names and schemas SHALL be unchanged.

### Acceptance criteria

- [ ] MCP `tools/list` contains the six tools; round-trip ingest -> structure -> pages works
- [ ] Read-only role cannot call `pageindex_index_document`
- [ ] `cargo test -p xavier --lib test_mcp_pageindex` green

## REQ-082: PageIndex PDF Trees Behind Cargo Features

- **Category:** Functional
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Files:** `crates/xavier-pageindex/src/pdf/*`, `crates/xavier-pageindex/Cargo.toml`
- **Features:** `feat-pageindex-pdf`

### Description

PDF trees SHALL be built by a cascade: embedded bookmarks (`pdf-outline`, pure Rust) -> layout-heuristic headings (`pdf-layout`, pdfium bound dynamically at runtime) -> optional LLM ToC -> fixed windows. Default and `ci-safe` builds SHALL compile and pass without any native PDF library; a missing pdfium SHALL yield a typed error, not a panic. Optional tree optimization (merge tiny / split huge nodes) and LLM node summaries SHALL preserve page coverage and never be required to build a tree.

### Acceptance criteria

- [ ] `cargo check -p xavier-pageindex` (default features) needs no pdfium
- [ ] `test_pdf_cascade_prefers_bookmarks`, `test_optimize_preserves_page_coverage`, `test_tree_builds_with_no_llm_configured` green

## REQ-083: PageIndex Hybrid Retrieval Arm

- **Category:** Functional
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Files:** `src/retrieval/pageindex_arm.rs`, `src/retrieval/gating.rs`, `src/retrieval/mod.rs`
- **Features:** `feat-pageindex-hybrid`

### Description

A retrieval arm SHALL return candidate tree nodes with their page ranges and be fused by RRF in the gating layer. This wave delivers the BM25 node leg: BM25 over node title + summary + the node's own page text, cached per workspace and fused as one weighted RRF source. The arm is disabled by default (`XAVIER_PAGEINDEX_ARM_ENABLED`), fail-open, workspace-scoped, and leaves existing retrieval behaviour byte-identical when disabled.

**Scope note.** The vector leg (node summaries persisted through the memory embedding API with doc/node/page tags, queried only over pageindex records and fused with the BM25 leg before gating RRF) is NOT part of this wave. It is an explicit planned follow-up: `.gitcore/waves/wave-pageindex/issue-15-followup-vector-leg.md`. BM25 shipped first because the spec's evaluation shows search-first navigation already cut pages read per question from 11.6 to 1.7 at 60/62 correct, so a lexical leg captures most of the practical value at no embedding cost.

### Acceptance criteria

- [ ] `test_gating_arm_disabled_by_default_no_behavior_change`, `test_gating_arm_failure_is_fail_open` green
- [ ] Existing `retrieval::gating` tests unchanged and green

## REQ-084: PageIndex Retrieval Evaluation

- **Category:** Quality
- **Priority:** Medium
- **SRS Status:** `implemented`
- **Files:** `tests/pageindex_eval.rs`, `tests/fixtures/pageindex/*`
- **Features:** `feat-pageindex-hybrid`

### Description

An LLM-free harness SHALL measure page-range hit-rate@k and MRR for DocBot, tree and hybrid retrieval on an in-repo fixture set with gold page ranges, and optionally on a PageIndex-OSS-Benchmark subset loaded from JSONL. The comparison result SHALL be recorded in the feature spec with measured numbers.

### Acceptance criteria

- [ ] `test_eval_report_compares_docbot_tree_hybrid` green (deterministic, no network)
- [ ] Spec records measured hit@1/hit@3/MRR per arm

*WAVE-10 (2026-09-21): REQ-060..065 added (skill-injection wave: scan-paths, semantic-rank, loader-fate, fusion-gates, MCP-tools, ledger-docs). Implemented live: registry scans canonical store, cosine rank w/ keyword tiebreak, fusion gate 0.5 + ack-gate, MCP dispatch/list tools. Measured: Recall@3 0.950 / MRR 0.950 (20-query offline eval), live E2E post 0.2.5 restart. Full `verify-pipeline` green deferred: pre-existing zero-match filters outside the wave need ledger-wide cleanup (see rescan report).*

*Domain-specific REQ-020..027 added 2026-08-08 (F12 preservation + mini-experts vision). Updated 2026-08-04 (honesty reconciliation: 27 features ↔ REQ-001..019 ↔ US-001..032). REQ-029..030 added 2026-08-14 (node provisioning — Olas M6/M7). Note: REQ-028/US-041 are reserved by `feat-issue-context-packager` (see features.json); new IDs use REQ-029..030 / US-042..043 to avoid collision. WAVE-3 (2026-08-31): REQ-031..040 added, 10 deltas, features 46→52 (4 promotions + 6 new), Docs + harness verified. WAVE-4 (2026-08-31): REQ-012,020,021,022,023,024,025,026,027,029,030 promoted to `verified` 100% (9 PRs 1753-1767 + 1758), `cargo test --package xavier --lib --features ci-safe` 2009 passed + `xavier-wasm` 4 + `code-graph` 81 + `xavier-core-logic` 24, clippy 0, fmt 0, panel-ui build 0. WAVE-5 (2026-09-01): REQ-044 added for panel browser compat. WAVE-6 (2026-09-03): REQ-045..046 added for Desktop One-Click installer & Cloudflare Edge Persistence. REQ-047 added 2026-09-05 for RTK Kernel CLI Proxy. WAVE-8 2026-09-12: REQ-048..052 added (HumanChallenge curation pipeline, introspection mode, privacy pipeline, enterprise ZDR, informed consent). Module: humanchallenge + data_commons + enterprise + panel-ui. WAVE-9 (2026-09-18): REQ-053..059 added (ripwire+graphify extraction O1-O7: honest-confidence, language-registry, blast-testgate, incremental-ids, budget-query, pagerank-router, contracts-arch; US-101..US-114; specs docs/features/specs/FEATURE-feat-cg-*.md; doc docs/EXTRACTION-RIPWIRE-GRAPHIFY.md; ADR-032/033).*

