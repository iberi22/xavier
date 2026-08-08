# User Stories — xavier

> **Protocol:** GitCore 3.8.0 · **Updated:** 2026-08-04
> Each user story links to its feature (`feat-*` in `.gitcore/features.json`) and SRS requirement (`REQ-NNN`).
> Format: `As a <role>, I want <capability>, so that <benefit>`.

## Index

| Story | Role | Feature | REQ |
|-------|------|---------|-----|
| US-001 | Agent operator | feat-unified-storage | REQ-005 |
| US-002 | Agent operator | feat-unified-storage | REQ-005 |
| US-003 | Agent operator | feat-hybrid-search | REQ-009 |
| US-004 | Agent operator | feat-belief-graph | REQ-009 |
| US-005 | Agent operator | feat-mcp-server | REQ-005 |
| US-006 | External agent | feat-mcp-server | REQ-010 |
| US-007 | Developer | feat-code-graph-index | REQ-011 |
| US-008 | Agent operator | feat-session-management | REQ-004 |
| US-009 | Security admin | feat-encryption-at-rest | REQ-006 |
| US-010 | Developer | feat-documentation-site | REQ-001 |
| US-011 | Developer | feat-src-reference | REQ-002 |
| US-012 | Mesh node operator | feat-mesh-network | REQ-012 |
| US-013 | Mesh node operator | feat-mesh-network | REQ-012 |
| US-014 | Operator | feat-telegram-bot | REQ-013 |
| US-015 | Operator | feat-notification-system | REQ-013 |
| US-016 | Agent operator | feat-hormer-navigation | REQ-016 |
| US-017 | Community member | feat-governance-dao | REQ-014 |
| US-018 | Council member | feat-governance-dao | REQ-014 |
| US-019 | Operator | feat-runtime-health | REQ-015 |
| US-020 | Researcher | feat-auto-improvement | REQ-016 |
| US-021 | Developer | feat-dual-license | REQ-018 |
| US-022 | Agent operator | feat-context-regeneration | REQ-016 |
| US-023 | Operator | feat-openclaw-scanner | REQ-019 |
| US-024 | Operator | feat-agent-cli-commands | REQ-019 |
| US-025 | Privacy user | feat-local-first | REQ-017 |
| US-026 | Agent operator | feat-token-savings | REQ-016 |
| US-027 | Developer | feat-plugin-system | REQ-011 |
| US-028 | Security admin | feat-security-hygiene | REQ-006 |
| US-029 | Dashboard user | feat-graph-explorer | REQ-011 |
| US-030 | Maintainer | feat-codegraph-maturity-bridge | REQ-011 |
| US-031 | SWAL node user | feat-decentralized-login | REQ-008 |
| US-032 | SWAL node user | feat-decentralized-login | REQ-003 |

---

## US-001: Durable memory persistence

As an **agent operator**, I want memories stored durably in SQLite so that **they survive restarts and are queryable later**.

- **Feature:** `feat-unified-storage` · **REQ:** REQ-005
- **Acceptance:** memory survives process restart; storage health OK in `/health`.

## US-002: Vector storage

As an **agent operator**, I want high-dimensional vectors stored with SQLite-vec so that **semantic search works locally**.

- **Feature:** `feat-unified-storage` · **REQ:** REQ-005
- **Acceptance:** `POST /v1/memories` stores vector rows; search retrieves them.

## US-003: Hybrid retrieval

As an **agent operator**, I want keyword + semantic hybrid search with RRF so that **queries return relevant results even with partial wording**.

- **Feature:** `feat-hybrid-search` · **REQ:** REQ-009
- **Acceptance:** BM25-only matches AND vector-only matches both surface in merged results.

## US-004: Belief graph inference

As an **agent operator**, I want inferred relationships between concepts so that **memory gains structure beyond flat records**.

- **Feature:** `feat-belief-graph` · **REQ:** REQ-009
- **Acceptance:** entity graph traversal returns related concepts; decay applied hourly.

## US-005: Memory search over MCP

As an **agent operator**, I want to search memory through the MCP tool so that **my agent runtime integrates without custom HTTP code**.

- **Feature:** `feat-mcp-server` · **REQ:** REQ-005
- **Acceptance:** `memory_search` tool returns ranked results over MCP.

## US-006: Standard MCP negotiation

As an **external agent**, I want protocol version negotiation so that **I can connect with any MCP-compatible client**.

- **Feature:** `feat-mcp-server` · **REQ:** REQ-010
- **Acceptance:** client and server negotiate 2024-11-05 protocol.

## US-007: Code symbol search

As a **developer**, I want to find symbols across my codebase so that **I can navigate large repos fast**.

- **Feature:** `feat-code-graph-index` · **REQ:** REQ-011
- **Acceptance:** `/code/find` returns symbols with kind/pattern filters.

## US-008: Session continuity

As an **agent operator**, I want sessions persisted and shareable so that **I can resume work across instances**.

- **Feature:** `feat-session-management` · **REQ:** REQ-004
- **Acceptance:** session export → import restores context tiers (shallow/medium/deep).

## US-009: Encrypted at rest

As a **security admin**, I want stored memories encrypted with AES-256-GCM so that **data at rest is unreadable without the key**.

- **Feature:** `feat-encryption-at-rest` · **REQ:** REQ-006
- **Acceptance:** encrypted bytes differ from plaintext; decrypt round-trips.

## US-010: Public documentation

As a **developer**, I want a docs site so that **new users can onboard quickly**.

- **Feature:** `feat-documentation-site` · **REQ:** REQ-001
- **Acceptance:** Starlight site builds and deploys.

## US-011: Source reference

As a **developer**, I want SRC.md to map the real codebase so that **I can find modules and commands without reading all code**.

- **Feature:** `feat-src-reference` · **REQ:** REQ-002
- **Acceptance:** every top-level module listed; build/test commands present.

## US-012: P2P memory sync

As a **mesh node operator**, I want memories synced between nodes so that **knowledge is shared across my devices**.

- **Feature:** `feat-mesh-network` · **REQ:** REQ-012
- **Acceptance:** two nodes exchange a memory record (Phase 0-1, HTTP transport).

## US-013: Mesh ACL enforcement

As a **mesh node operator**, I want deep permissions so that **untrusted nodes cannot read my private memories**.

- **Feature:** `feat-mesh-network` · **REQ:** REQ-012
- **Acceptance:** denied peer cannot fetch restricted namespaces (ACL 90%).

## US-014: Telegram control

As an **operator**, I want to query memory from Telegram so that **I can use Xavier on the go**.

- **Feature:** `feat-telegram-bot` · **REQ:** REQ-013
- **Acceptance:** `/memory search <q>` returns results (MVP; standalone bot residual).

## US-015: Event notifications

As an **operator**, I want email/webhook/in-app notifications so that **I learn of events without polling**.

- **Feature:** `feat-notification-system` · **REQ:** REQ-013
- **Acceptance:** 3 channels deliver; SQLite persists; REST API reads them.

## US-016: Hierarchical navigation

As an **agent operator**, I want navigation-aware recall so that **context retrieval follows a learned policy**.

- **Feature:** `feat-hormer-navigation` · **REQ:** REQ-016
- **Acceptance:** HORMER shell commands return focused context.

## US-017: Community voting

As a **community member**, I want to propose and vote on XIPs so that **I influence project direction**.

- **Feature:** `feat-governance-dao` · **REQ:** REQ-014
- **Acceptance:** proposal moves Draft→Discussion→Voting with reputation-weighted tally.

## US-018: Council veto

As a **council member**, I want veto power on security-critical changes so that **the network stays safe**.

- **Feature:** `feat-governance-dao` · **REQ:** REQ-014
- **Acceptance:** 66% council vote vetoes; community 75% can overrule.

## US-019: Health visibility

As an **operator**, I want a health endpoint so that **I can monitor Xavier at a glance**.

- **Feature:** `feat-runtime-health` · **REQ:** REQ-015
- **Acceptance:** `/health` returns DB/embedding/LLM/mesh status (verified 2026-08-04).

## US-020: Auto-improvement loop

As a **researcher**, I want benchmark → gap → fix automation so that **retrieval quality improves without manual tuning**.

- **Feature:** `feat-auto-improvement` · **REQ:** REQ-016
- **Acceptance:** `xavier improve` detects low recall and proposes experiments (Phase 1).

## US-021: License choice

As a **developer**, I want to choose MIT vs Mesh license so that **I can use Xavier standalone or join the network**.

- **Feature:** `feat-dual-license` · **REQ:** REQ-018
- **Acceptance:** `xavier license accept` upgrades; gate enforces mesh features.

## US-022: Perfect recall

As an **agent operator**, I want regenerated context so that **long sessions don't degrade recall quality**.

- **Feature:** `feat-context-regeneration` · **REQ:** REQ-016
- **Acceptance:** recall@k improves on production benchmark after regeneration.

## US-023: OpenClaw ingestion

As an **operator**, I want to scan OpenClaw agent files so that **their memory becomes searchable in Xavier**.

- **Feature:** `feat-openclaw-scanner` · **REQ:** REQ-019
- **Acceptance:** scan finds MEMORY.md/SOUL.md/USER.md; indexer embeds them.

## US-024: Agent CLI

As an **operator**, I want `xavier agent scan|index|push|pull|status|sync` so that **I manage agent memory from the terminal**.

- **Feature:** `feat-agent-cli-commands` · **REQ:** REQ-019
- **Acceptance:** each subcommand completes; JSON output parseable.

## US-025: Fully local operation

As a **privacy-conscious user**, I want all LLM/embedding calls local so that **no data leaves my machine**.

- **Feature:** `feat-local-first` · **REQ:** REQ-017
- **Acceptance:** with Ollama up, `/health` shows provider=local healthy.

## US-026: Token savings

As an **agent operator**, I want index-first MCP search so that **agent token usage drops ~90%**.

- **Feature:** `feat-token-savings` · **REQ:** REQ-016
- **Acceptance:** `measure_token_savings.py` reports ≥90% measured in env.

## US-027: Plugin extensibility

As a **developer**, I want to load code-graph plugins so that **I can extend indexing without forking**.

- **Feature:** `feat-plugin-system` · **REQ:** REQ-011
- **Acceptance:** PluginManager loads ProcessEngine/NativeEngine; parser-python release (residual).

## US-028: Dependency hygiene

As a **security admin**, I want dependabot alerts triaged so that **main has no unmitigated high-severity vulns**.

- **Feature:** `feat-security-hygiene` · **REQ:** REQ-006
- **Acceptance:** inventory in SECURITY_DEPENDABOT.md; residual transitives tracked in #478.

## US-029: Graph exploration

As a **dashboard user**, I want multi-layer graphs (roadmap/memory/code) so that **I can visualize Xavier's knowledge**.

- **Feature:** `feat-graph-explorer` · **REQ:** REQ-011
- **Acceptance:** panel shows memory KG + code force-graph layers (Windows smoke PASS).

## US-030: Maturity bridge

As a **maintainer**, I want codegraph → maturity/docs alignment so that **auto-docs reflect indexed reality**.

- **Feature:** `feat-codegraph-maturity-bridge` · **REQ:** REQ-011
- **Acceptance:** maturity Layer1 prefers SQLite, falls back to JSON dump then grep.

## US-031: Node identity

As a **SWAL node user**, I want a local identity without accounts so that **I own my node key**.

- **Feature:** `feat-decentralized-login` · **REQ:** REQ-008
- **Acceptance:** BIP39-24 + Shamir 2-of-3 vault; seed never logged.

## US-032: Pro via active node

As a **SWAL node user**, I want Pro features unlocked by an active node so that **I don't need subscriptions or Stripe**.

- **Feature:** `feat-decentralized-login` · **REQ:** REQ-003
- **Acceptance:** `pro_gate` permits when node heartbeat active; denies otherwise.

---

## US-033: Classify information by sensitivity level

As a **curator**, I want to classify information (UNCLASSIFIED→TOPSECRET) so that **sensitive sections are only visible to authorized nodes**.

- **Feature:** `feat-clearance-levels` · **REQ:** REQ-020
- **Acceptance:** `ClearanceLevel` enum; read middleware redacts by requester clearance.

## US-034: Create information groups with permissions

As a **node owner**, I want to organize information in groups with read/write/audit permissions so that **only members access each group**.

- **Feature:** `feat-groups-permissions` · **REQ:** REQ-021
- **Acceptance:** ACL enforced on all reads; bypass attempts blocked and logged.

## US-035: Export my data for training

As a **user**, I want Xavier to serve my curated data as train/eval datasets so that **I can train a personal model with my own data**.

- **Feature:** `feat-training-datasets-api` · **REQ:** REQ-022
- **Acceptance:** `/v1/training/datasets/{id}/train` returns JSONL with consent-filtered records.

## US-036: Train a personal mini-expert

As a **user**, I want to train a small model with my own language and my segment data so that **I get an on-demand expert that loads fast**.

- **Feature:** `feat-mini-experts` · **REQ:** REQ-023
- **Acceptance:** Pipeline dataset → Colab/Vertex → GGUF → served locally; model responds in user's language.

## US-037: Share work telemetry with the service network

As a **SWAL service node**, I want to share benchmarks/logs/feedbacks (never personal data) so that **the network improves Xavier collectively**.

- **Feature:** `feat-mesh-service-network` · **REQ:** REQ-024
- **Acceptance:** Telemetry classified INTERNAL; personal data excluded (tests).

## US-038: Private mesh across my devices

As a **user**, I want my devices (same key wallet) to form a private mesh so that **my memory and models sync privately**.

- **Feature:** `feat-mesh-private-wallet` · **REQ:** REQ-025
- **Acceptance:** Same-wallet nodes discover each other; other wallets cannot see the mesh.

## US-039: Read redacted documents

As a **low-clearance reader**, I want sensitive sections shown as REDACTED so that **I get the useful parts without seeing secrets**.

- **Feature:** `feat-content-redaction` · **REQ:** REQ-026
- **Acceptance:** Secret section of a document hidden at low clearance; full version at high clearance.

## US-040: Curate information as a human

As a **human curator**, I want to review/approve/classify information so that **the future is built with real regenerated info, not generated**.

- **Feature:** `feat-human-curation` · **REQ:** REQ-027
- **Acceptance:** Curation flow with approval; personal models train only on curated data.

---

*Domain-specific US-033..040 added 2026-08-08 (F12 preservation + mini-experts vision). Updated 2026-08-04.*
