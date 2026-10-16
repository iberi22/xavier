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
| US-044 | As a user, I want to enter introspection sessions surfaced by Xavier | REQ-049 |
| US-045 | As a user, I want my training data scrubbed of PII before any export | REQ-050 |
| US-101 | Agent operator | feat-cg-honest-confidence | REQ-053 |
| US-102 | System auditor | feat-cg-honest-confidence | REQ-053 |
| US-103 | Language maintainer | feat-cg-language-registry | REQ-054 |
| US-104 | Developer | feat-cg-language-registry | REQ-054 |
| US-105 | Developer | feat-cg-blast-testgate | REQ-055 |
| US-106 | Qa engineer | feat-cg-blast-testgate | REQ-055 |
| US-107 | Database administrator | feat-cg-incremental-ids | REQ-056 |
| US-108 | Developer | feat-cg-incremental-ids | REQ-056 |
| US-109 | Api consumer | feat-cg-budget-query | REQ-057 |
| US-110 | System designer | feat-cg-budget-query | REQ-057 |
| US-111 | Data scientist | feat-cg-pagerank-router | REQ-058 |
| US-112 | Search user | feat-cg-pagerank-router | REQ-058 |
| US-113 | Code reviewer | feat-cg-contracts-arch | REQ-059 |
| US-114 | Software architect | feat-cg-contracts-arch | REQ-059 |
| US-115 | Agent operator | feat-skill-scan-paths | REQ-060 |
| US-116 | Agent user | feat-skill-semantic-rank | REQ-061 |
| US-117 | Skill author | feat-skill-loader-fix | REQ-062 |
| US-118 | User | feat-skill-context-fusion | REQ-063 |
| US-119 | External agent | feat-skill-mcp-tool | REQ-064 |
| US-120 | Project manager | feat-skill-ledger-docs | REQ-065 |

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

## US-042: Provision a BaaS cloud node with my token

As a **user**, I can register a cloud service as a SWAL node by pasting its API token (Supabase/Neon), so that **Xavier provisions and administers it autonomously**.

- **Feature:** `feat-node-provisioning` · **REQ:** REQ-029
- **Acceptance:** `xavier nodes add --provider supabase --token sbp_xxx` provisions RLS + encrypted bucket + heartbeat; token lives only in Clavis (AES-256-GCM + lease TTL); node registers public (M1) or private (M3) per visibility.

## US-043: Register my VPS as a private node

As a **user**, I can register my VPS as a private SWAL node over SSH, so that **my private mesh info persists on my own infrastructure under my wallet's control**.

- **Feature:** `feat-node-provisioning` · **REQ:** REQ-030
- **Acceptance:** `xavier nodes add --provider vps --ssh user@host --key ~/.ssh/id_ed25519` installs edge-hive lite; SSH key only in Clavis; node syncs memory+snapshots with session encryption; other wallets cannot join.

---

## US-044: Guided Introspection Sessions

**As a** Xavier user,
**I want to** browse available introspection sessions detected from my interactions and enter them voluntarily,
**So that** I can engage in guided deep reflection on decisions and assumptions using professional psychological techniques.

**Acceptance Criteria:**
- [ ] AC-044.1: Dashboard shows available sessions with technique, topic preview, and depth indicator
- [ ] AC-044.2: User can enter a session with a single click — LLM immediately begins as guide
- [ ] AC-044.3: LLM never gives direct answers — only facilitating questions
- [ ] AC-044.4: Session insights are extractable as curated training data (with consent)
- [ ] AC-044.5: Session state persists — user can resume incomplete sessions

**Priority:** P2 — Medium | **Effort:** L | **REQ:** REQ-049 | **Wave:** 8

---

## US-045: Privacy-Safe Training Data Export

**As a** Xavier user,
**I want my** challenge responses and introspection insights to be automatically scrubbed of personal information before being used for model training,
**So that** I can participate in training improvement without exposing private data.

**Acceptance Criteria:**
- [ ] AC-045.1: Emails, file paths, and API keys are replaced with [EMAIL], [PATH], [KEY] tokens
- [ ] AC-045.2: P3 exports (Colab) additionally apply differential privacy noise to confidence scores
- [ ] AC-045.3: Export is rejected if k-anonymity (k>=3) cannot be satisfied
- [ ] AC-045.4: Every exported bundle includes a privacy audit report in its manifest
- [ ] AC-045.5: User sees privacy level label (P2/P3/P4) before confirming export

**Priority:** P1 — High | **Effort:** M | **REQ:** REQ-050, REQ-052 | **Wave:** 8

---

US-046: retired 2026-10-15.

---

## US-101: Code-Graph Honest Confidence (O1)

As a **agent operator**, I want a distinct confidence rubric and labeled honesty signals on code graph queries so that **I can reliably trust automated inferences vs exact extractions**.

- **Feature:** `feat-cg-honest-confidence` · **REQ:** REQ-053
- **Acceptance:** Single confidence rubric (EXTRACTED 1.0 / INFERRED 0.95-0.55 / AMBIGUOUS) plus labeled honesty (floors, caps, amb, refuse-vs-empty) on all graph answers. Sources: ripwire R1 + graphify G1. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md, ADR-032.

## US-102: Code-Graph Honest Confidence (O1)

As a **system auditor**, I want ambiguity markers and refusal-vs-empty explanations in graph answers so that **I can understand why certain relationships could not be definitively resolved**.

- **Feature:** `feat-cg-honest-confidence` · **REQ:** REQ-053
- **Acceptance:** Single confidence rubric (EXTRACTED 1.0 / INFERRED 0.95-0.55 / AMBIGUOUS) plus labeled honesty (floors, caps, amb, refuse-vs-empty) on all graph answers. Sources: ripwire R1 + graphify G1. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md, ADR-032.

## US-103: Code-Graph Language Registry (O2)

As a **language maintainer**, I want a unified LanguageConfig and suffix-ordered resolver registry so that **I can easily plug in support for new languages without touching the core indexer**.

- **Feature:** `feat-cg-language-registry` · **REQ:** REQ-054
- **Acceptance:** Unified LanguageConfig plus suffix-ordered resolver registry so new languages plug in without touching indexer core. Source: graphify G2. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md.

## US-104: Code-Graph Language Registry (O2)

As a **developer**, I want the code graph to auto-detect language configurations via suffix ordering so that **my diverse polyglot projects are indexed seamlessly**.

- **Feature:** `feat-cg-language-registry` · **REQ:** REQ-054
- **Acceptance:** Unified LanguageConfig plus suffix-ordered resolver registry so new languages plug in without touching indexer core. Source: graphify G2. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md.

## US-105: Code-Graph Blast Radius + Test Gate (O3)

As a **developer**, I want a transitive blast radius mapping with use roles so that **I instantly know which components might break due to my code changes**.

- **Feature:** `feat-cg-blast-testgate` · **REQ:** REQ-055
- **Acceptance:** Transitive blast radius with use roles plus tests-to-run mapping and pre-merge test gate (exit 4 on untested radius). Source: ripwire R2. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md.

## US-106: Code-Graph Blast Radius + Test Gate (O3)

As a **qa engineer**, I want a pre-merge test gate that maps blast radius to tests-to-run (exit 4 on untested radius) so that **I can enforce safety standards and prevent unverified regressions**.

- **Feature:** `feat-cg-blast-testgate` · **REQ:** REQ-055
- **Acceptance:** Transitive blast radius with use roles plus tests-to-run mapping and pre-merge test gate (exit 4 on untested radius). Source: ripwire R2. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md.

## US-107: Code-Graph Incremental IDs (O4)

As a **database administrator**, I want real multi-project stable IDs so that **I can reference exact graph nodes deterministically across environments**.

- **Feature:** `feat-cg-incremental-ids` · **REQ:** REQ-056
- **Acceptance:** Real multi-project stable IDs plus content-hash incremental reindex plus 2-phase call resolution with god-guard. Sources: graphify G3+G4. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md, ADR-033.

## US-108: Code-Graph Incremental IDs (O4)

As a **developer**, I want content-hash incremental reindexing so that **subsequent indexing cycles are fast and only process modified files**.

- **Feature:** `feat-cg-incremental-ids` · **REQ:** REQ-056
- **Acceptance:** Real multi-project stable IDs plus content-hash incremental reindex plus 2-phase call resolution with god-guard. Sources: graphify G3+G4. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md, ADR-033.

## US-109: Code-Graph Budget Query (O5)

As a **api consumer**, I want a strict token budget with early gates so that **I can confidently run intensive queries without exceeding LLM context windows**.

- **Feature:** `feat-cg-budget-query` · **REQ:** REQ-057
- **Acceptance:** Token budget with gate plus cheap pre-vector query (vocab expansion, bounded BFS/DFS). Sources: ripwire R3 + graphify G6. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md.

## US-110: Code-Graph Budget Query (O5)

As a **system designer**, I want cheap pre-vector queries like bounded BFS/DFS and vocab expansion so that **I can efficiently filter large graphs before doing costly vector distance calculations**.

- **Feature:** `feat-cg-budget-query` · **REQ:** REQ-057
- **Acceptance:** Token budget with gate plus cheap pre-vector query (vocab expansion, bounded BFS/DFS). Sources: ripwire R3 + graphify G6. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md.

## US-111: Code-Graph PageRank Router (O6)

As a **data scientist**, I want structural graph ranking with deterministic PageRank so that **the most central and critical nodes are naturally prioritized**.

- **Feature:** `feat-cg-pagerank-router` · **REQ:** REQ-058
- **Acceptance:** Structural ranking (edge weights plus deterministic PageRank) plus lexical router with confidence margin. Source: ripwire R4. Needs petgraph as direct dependency. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md.

## US-112: Code-Graph PageRank Router (O6)

As a **search user**, I want a lexical router enriched with confidence margins from structural ranking so that **my search results intelligently blend keyword relevance with architectural importance**.

- **Feature:** `feat-cg-pagerank-router` · **REQ:** REQ-058
- **Acceptance:** Structural ranking (edge weights plus deterministic PageRank) plus lexical router with confidence margin. Source: ripwire R4. Needs petgraph as direct dependency. See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md.

## US-113: Code-Graph Contracts + Architecture Signals (O7)

As a **code reviewer**, I want pre-merge contracts (edit-check, safe-delete, 3-valued verify) so that **I get immediate, evidence-backed verdicts on code safety during PR review**.

- **Feature:** `feat-cg-contracts-arch` · **REQ:** REQ-059
- **Acceptance:** Pre-merge contracts (edit-check, safe-delete, 3-valued verify) plus architecture signals (god-nodes, surprising links, import cycles, rationale nodes). Sources: ripwire R5 + graphify G7 (+G5 Leiden as optional O8). See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md.

## US-114: Code-Graph Contracts + Architecture Signals (O7)

As a **software architect**, I want automated architecture signals (god-nodes, surprising links, import cycles) so that **I can proactively spot structural decay and technical debt**.

- **Feature:** `feat-cg-contracts-arch` · **REQ:** REQ-059
- **Acceptance:** Pre-merge contracts (edit-check, safe-delete, 3-valued verify) plus architecture signals (god-nodes, surprising links, import cycles, rationale nodes). Sources: ripwire R5 + graphify G7 (+G5 Leiden as optional O8). See docs/EXTRACTION-RIPWIRE-GRAPHIFY.md.

## US-115: Skill Registry Scans Canonical Store

As a **agent operator**, I want the SkillRegistry to safely scan ~/.hermes/skills while respecting disabled lists so that **I have an up-to-date catalog of active local skills without symlink loops**.

- **Feature:** `feat-skill-scan-paths` · **REQ:** REQ-060
- **Acceptance:** SkillRegistry indexes ~/.hermes/skills (canonical 349-skill store) with symlink-cycle-safe traversal and Hermes disabled-list respect

## US-116: Semantic Skill Ranking via Local Embeddings

As a **agent user**, I want skills to be semantically ranked using local SQLite vector embeddings so that **the right skill is surfaced exactly when I need it based on meaning, not just keywords**.

- **Feature:** `feat-skill-semantic-rank` · **REQ:** REQ-061
- **Acceptance:** Rank skills by cosine similarity over local embeddings (EmbeddingPort + sqlite-vec) with keyword score as tiebreak and calibrated 0-1 confidence

## US-117: SkillLoader Frontmatter Validation or Removal

As a **skill author**, I want strict frontmatter validation for skills so that **I get clear feedback if my skill definition is improperly formatted**.

- **Feature:** `feat-skill-loader-fix` · **REQ:** REQ-062
- **Acceptance:** Decide SkillLoader fate: validate real SKILL.md frontmatter (name:) or delete module and migrate its tests to the registry

## US-118: Session Context Fusion Injects Matched Skills

As a **user**, I want top-matched skills to be automatically injected via session context fusion so that **the agent acts with appropriate capabilities immediately without manual prompting**.

- **Feature:** `feat-skill-context-fusion` · **REQ:** REQ-063
- **Acceptance:** session/compact path injects top-1 skill content within remaining token budget on ContextLevel::Maximum with confidence >= 0.5, injects nothing below threshold or on trivial prompts

## US-119: MCP Skill Dispatch Tools

As a **external agent**, I want MCP tools like xavier_dispatch_skill and xavier_skill_list so that **I can programmatically discover and invoke Xavier skills over standard MCP**.

- **Feature:** `feat-skill-mcp-tool` · **REQ:** REQ-064
- **Acceptance:** Expose xavier_dispatch_skill {task, max_tokens?, project?} and xavier_skill_list MCP tools wrapping the same dispatcher as the REST endpoints

## US-120: Skill Wave Ledger, SRS and Ack-Gate Docs

As a **project manager**, I want the Skill Wave Ledger to generate formal SRS and ack-gate documentation so that **we maintain 100% traceability from raw features to user requirements**.

- **Feature:** `feat-skill-ledger-docs` · **REQ:** REQ-065
- **Acceptance:** Add REQ-060..065 to SRS, polish wave specs, document local trivial-prompt ack gate; close wave only on green verify-pipeline

*Domain-specific US-033..040 added 2026-08-08 (F12 preservation + mini-experts vision). Updated 2026-08-04. US-042..043 added 2026-08-14 (node provisioning — Olas M6/M7). US-041 reserved by `feat-issue-context-packager`.*
