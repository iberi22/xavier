# Software Requirements Specification — xavier

> **Protocol:** GitCore 3.8.0 · **Updated:** 2026-08-04
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
- [ ] Maloca UI `obtainDeviceKeyViaWebAuthn` (product)

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
- **SRS Status:** `draft` (**45%** — Phase 0-1 only)
- **Files:** `src/mesh/` (38 files), `src/data_commons/`, `tests/mesh_integration.rs`
- **Features:** `feat-mesh-network` (45%) · EPIC: #115

### Description
Distributed P2P memory sync: Ed25519 identity, encrypted transport, ACL, Data Commons. Phase 2+ (Iroh/QUIC NAT traversal, Loro CRDT, Tor) not started.

### Acceptance criteria
- [x] Node identity + pairing codes (Phase 0)
- [x] Memory sync protocol (HTTP transport, 100%)
- [x] ACL / Deep Permissions (90%)
- [x] Tokenomics scaffolding (40%)
- [ ] libp2p transport compiles & connects (currently **10%, broken legacy**)
- [ ] On-chain governance in mesh (0%)
- [ ] Phase 2: Iroh/QUIC with NAT traversal
- [ ] Phase 3: Loro CRDT conflict-free merge
- [ ] Phase 4: Tor/Yggdrasil transport
- [ ] Active peers > 0 in `/health` (currently 0)

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
- **SRS Status:** `planned`
- **Features:** `feat-clearance-levels`
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §3

### Description
Classify documents like government classified material: UNCLASSIFIED → TOPSECRET (6 levels). Server redacts sections by requester clearance; no bypass.

### Acceptance criteria
- [ ] `ClearanceLevel` enum (0-5) with serialization
- [ ] `clearance` field on MemoryRecord + DatasetMetadata
- [ ] Read middleware redacts by requester clearance
- [ ] Per-section REDACTED support within a document
- [ ] Access audit log (who/what/when/clearance)

---

## REQ-021: Information groups with strict permissions

- **Category:** Security
- **Priority:** High
- **SRS Status:** `planned`
- **Features:** `feat-groups-permissions`
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §4

### Description
Information groups (core-xavier-dev, service-nodes, family) with rigorous ACL (read/write/audit) enforced on ALL reads, no bypass, audited.

### Acceptance criteria
- [ ] Group model + membership
- [ ] ACL per group (read/write/audit roles)
- [ ] Enforcement in all server reads
- [ ] Audit trail of accesses
- [ ] Bypass-attempt tests

---

## REQ-022: Training datasets API

- **Category:** Data
- **Priority:** High
- **SRS Status:** `partial`
- **Features:** `feat-training-datasets-api`
- **Files:** `src/data_commons/training.rs` (TrainingExporter exists)

### Description
Serve training datasets over REST: /v1/training/datasets + train/eval splits (JSONL) with consent audit, clearance, segment, language metadata.

### Acceptance criteria
- [ ] `GET /v1/training/datasets` — list
- [ ] `GET /v1/training/datasets/{id}` — manifest
- [ ] `GET /v1/training/datasets/{id}/train` + `/eval` — JSONL splits
- [ ] `POST /v1/training/bundles` — generate with seed/eval_ratio
- [ ] Metadata: clearance, consent, segment, language

---

## REQ-023: Personal mini-experts (on-demand local models)

- **Category:** AI
- **Priority:** Medium
- **SRS Status:** `planned`
- **Features:** `feat-mini-experts`
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §5

### Description
Small models (1-3B) trained with the user's own curated data, only the user's language (or EN+user), served locally via Ollama. Pipeline: dataset → Colab/Vertex (agy) → GGUF → local serve.

### Acceptance criteria
- [ ] Dataset export via /v1/training/*
- [ ] Colab/Vertex training pipeline (agy CLI)
- [ ] GGUF conversion + Ollama/llama.cpp serving
- [ ] Mini-expert registry (segment, language, clearance, source dataset)
- [ ] ProviderRouter includes local mini-experts

---

## REQ-024: SWAL service network (internal telemetry)

- **Category:** Mesh
- **Priority:** Medium
- **SRS Status:** `planned`
- **Features:** `feat-mesh-service-network`
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §2 Capa 2

### Description
Share benchmarks, logs, feedbacks, operational telemetry among service nodes to improve Xavier — strictly NO personal data. Classified INTERNAL.

### Acceptance criteria
- [ ] Telemetry classified INTERNAL
- [ ] Publish telemetry to service network
- [ ] Service nodes consume to improve Xavier
- [ ] Personal data exclusion guaranteed (tests)

---

## REQ-025: Private mesh by key wallet

- **Category:** Mesh
- **Priority:** Medium
- **SRS Status:** `planned`
- **Features:** `feat-mesh-private-wallet`
- **Design:** `docs/design/F9-MESH-SWAL-PUBLICO-PRIVADO.md` §3.7

### Description
Nodes anchored to the SAME key wallet form a private mesh: sync memory, snapshots, models across the user's devices. Third parties cannot see it.

### Acceptance criteria
- [ ] Node registration by wallet (Clavis)
- [ ] Private discovery (same wallet only)
- [ ] Memory + snapshot + model sync between devices
- [ ] Session encryption between private nodes
- [ ] Cross-wallet isolation tests

---

## REQ-026: Content redaction (partial censorship)

- **Category:** Security
- **Priority:** High
- **SRS Status:** `planned`
- **Features:** `feat-content-redaction`
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §3

### Description
Documents with REDACTED sections: server serves censored version per requester clearance, like government classified documents.

### Acceptance criteria
- [ ] Segmented document format (sections with levels)
- [ ] Per-section redaction engine
- [ ] Redacted vs full version serving by clearance
- [ ] Redaction tests (secret section hidden at low clearance)

---

## REQ-027: Human curation of information

- **Category:** Governance
- **Priority:** Medium
- **SRS Status:** `planned`
- **Features:** `feat-human-curation`
- **Design:** `docs/design/F12-PRESERVACION-MINI-EXPERTOS.md` §1

### Description
Humans are curators: review, approve, classify the information Xavier preserves. Personal models train ONLY on curated info. Real regenerated info, not generated.

### Acceptance criteria
- [ ] UI/API to review unclassified information
- [ ] Human approval flow (curate = classify + validate)
- [ ] Curation history (who classified what, when)
- [ ] Personal models trained only on curated data

---

## REQ-029: SWAL node provisioning (BaaS tokens — Supabase/Neon)

- **Category:** Mesh
- **Priority:** High
- **SRS Status:** `planned`
- **Features:** `feat-node-provisioning`
- **Design:** `docs/design/F9-MESH-SWAL-PUBLICO-PRIVADO.md` §3.8 (Ola M6)

### Description
Register a cloud service as a SWAL node by pasting its API token (Supabase/Neon). Xavier provisions and administers it autonomously via the provider API (RLS policies, encrypted buckets, edge functions relay/heartbeat; Neon schema + replication). The token lives ONLY in `src/secrets/` (LocalSecretsVault/HardwareVault AES-256-GCM persistente) + `KeyLendingEngine`/`EphemeralLease` (TTL/revoke) — never plaintext on disk/config/logs. The BaaS node registers in the public directory (M1) or the private mesh (M3) per visibility. Public SWAL info replicates to local mesh nodes via Yjs CRDT. *(Revisado 2026-08-14: validación Kimi — SecretLease → EphemeralLease; rotación de tokens BaaS requiere token nuevo del usuario, nunca generación local; revocación incluye deprovisioning remoto.)*

### Acceptance criteria
- [ ] `xavier nodes add --provider supabase --token sbp_xxx` provisions RLS + encrypted bucket + edge functions (relay/heartbeat)
- [ ] `xavier nodes add --provider neon --token npx_xxx` creates node schema + replication
- [ ] Token stored ONLY in `src/secrets/` (LocalSecretsVault/HardwareVault AES-256-GCM persistente + EphemeralLease UUID/TTL); test asserts no plaintext on disk/config/logs
- [ ] **Reinicio de Xavier: token del nodo sigue disponible** (persistencia real, no en memoria) — test de sobrevivencia a restart
- [ ] `xavier nodes rotate {id}` = usuario provee token NUEVO (o Xavier lo emite vía management API del provider); lease anterior revocado; **nunca** generación local `clavis_{name}_{uuid}`
- [ ] `xavier nodes remove {id}` → **deprovisioning remoto**: revoca token vía API del provider + deregistra (M1/M3); si la revocación remota falla → reporta "revocación parcial", nunca éxito falso
- [ ] Public BaaS node appears in `GET /mesh/public/nodes`; private BaaS node invisible to other wallets
- [ ] Supabase as persistent public admin node: `node_registry` (RLS anon READ, **write SOLO vía edge function que verifica firma Ed25519 del heartbeat contra node_id = hash(pubkey)**), `ops_feed` (public, mesh-replicable, **updates Yjs firmados + vector clock anti-rollback**), bucket `swal-vault` (private, E2E-encrypted JSON)
- [ ] Public mesh info syncs to local mesh nodes via Yjs CRDT (ops_feed = store&forward relay, not authority)
- [ ] Token en CLI `--token` solo para tests con mocks; en producción se lee de stdin/prompt/`XAVIER_NODE_TOKEN` (sin shell history ni `ps`)
- [ ] Eventos add/rotate/remove quedan en audit log estructurado append-only con masking

---

## REQ-030: SSH/VPS private nodes

- **Category:** Mesh
- **Priority:** High
- **SRS Status:** `planned`
- **Features:** `feat-node-provisioning`
- **Design:** `docs/design/F9-MESH-SWAL-PUBLICO-PRIVADO.md` §3.9 (Ola M7)

### Description
Register a VPS as a private SWAL node over SSH. Xavier **genera un keypair SSH dedicado por nodo** (nunca importa la clave personal del usuario), stores it in `src/secrets/` (never plaintext), installs the node agent (edge-hive lite, verificación de host key TOFU + checksum firmado), and registers it in the user's key wallet via certificado de nodo firmado por la billetera. The private node persists the user's internal mesh info (memory + snapshots) with session encryption. Permission inheritance: the wallet governs what replicates and with what encryption. *(Revisado 2026-08-14: validación Kimi — keypair dedicado, host key pinning, certificado de nodo = aislamiento cross-wallet.)*

### Acceptance criteria
- [ ] `xavier nodes add --provider vps --ssh user@host` **genera keypair dedicado por nodo**, instala SOLO la pubkey vía acceso existente, instala edge-hive lite y registra en la wallet
- [ ] **Prohibido** `--key ~/.ssh/id_ed25519` (clave personal): rechazo explícito si se intenta importar
- [ ] SSH key stored ONLY in `src/secrets/` (AES-256-GCM + lease TTL); test asserts no plaintext on disk
- [ ] **Host key pinning**: fingerprint del host verificado en provisioning (TOFU) y en cada conexión; flag `--host-key` para pinning estricto
- [ ] Node registers via Ed25519 challenge-response (M3 protocol) **con certificado de nodo firmado por la billetera** `(node_pubkey + node_id + expiry)`; default visibility `private`
- [ ] Private node syncs memory + snapshots of the internal mesh with session encryption (MeshSessionShare)
- [ ] Permission inheritance: wallet ACL governs what replicates and with what encryption
- [ ] `xavier nodes remove {id}` revoca el lease SSH **y ejecuta teardown**: desinstala agente + borra pubkey dedicada de `authorized_keys`; si falla → "revocación parcial"; **re-key de mesh** (nueva epoch de clave de sesión para nodos restantes)
- [ ] Cross-wallet isolation test: a node from another wallet cannot join the private mesh (certificado inválido rechazado en handshake)

---

*Domain-specific REQ-020..027 added 2026-08-08 (F12 preservation + mini-experts vision). Updated 2026-08-04 (honesty reconciliation: 27 features ↔ REQ-001..019 ↔ US-001..032). REQ-029..030 added 2026-08-14 (node provisioning — Olas M6/M7). Note: REQ-028/US-041 are reserved by `feat-issue-context-packager` (see features.json); new IDs use REQ-029..030 / US-042..043 to avoid collision.*
