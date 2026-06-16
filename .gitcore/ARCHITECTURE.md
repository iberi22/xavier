# Architecture: Xavier

## Core Philosophy
Xavier is a single Rust binary acting as a multi-agent cognitive memory swarm, inspired by the **"System 3" paradigm** (Rational Thought, Meta-Cognition, and Error Correction). This system transcends simple vector retrieval by implementing strict reasoning and self-reflection layers before serving responses.

Xavier is not a standalone RAG silo. It is the memory and reasoning substrate for **agentic workflows** — specifically designed as the memory backend for OpenClaw agents. The architecture includes a **P2P mesh network** for distributed memory sync, **tokenomics-based governance**, and a **runtime health loop** for continuous self-improvement.

## Product Direction
Xavier has five concentric layers:

1. **Core Memory** — QMD + Belief Graph + Hybrid Search (BM25 + Vector)
2. **Mesh Network** — P2P nodes with Ed25519 identity, encrypted transport, Data Commons
3. **Runtime Health** — Self-monitoring system for DB, embeddings, disk, mesh peers, with auto-benchmarking
4. **Governance** — Bicameral DAO (50% users + 50% core council), token-weighted voting, XP rewards
5. **Auto-Improvement** — Closed loop: benchmark → gap analysis → experiment → PR → measure

## Tech Stack
- **Language**: Rust
- **Runtime**: Tokio (for massive asynchronous parallelism across agent swarms)
- **Framework**: `zavora-ai/adk-rust` (Agent Development Kit for agnostic, modular agent building)
- **Database / Memory**: SQLite + SQLite-vec for durable shared memory and vector search, plus `QmdMemory` for in-process retrieval workflows
- **Code Index**: `code-graph` SQLite sidecar for AST/symbol indexing exposed through `/code/*`
- **Mesh Transport**: HTTP REST (Phase 1) → Iroh/QUIC (Phase 2) — Ed25519 identity, encrypted sync
- **Web Packages**: React/Vite panel client in `panel-ui` and Astro docs site in `docs/site`
- **Advanced Techniques**: BM25 Hybrid Search, MCP, QMD Memory, Belief Graphs, HORMER navigation

## Agent Swarm Layers (System 1-2-3)
1. **System 1 (Retrieval)**: Fast instinct-like agents powered by shared memory, lexical retrieval, and code indexing
2. **System 2 (Reasoning)**: Deliberate agents implementing Chain of Thought (CoT) based on System 1's context
3. **System 3 (Action / Oversight)**: Meta-cognitive agents that overrule and evaluate System 2's reasoning

## Mesh & Governance Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                     Xavier Mesh Network                       │
├────────────────────────────┬─────────────────────────────────┤
│  Node Identity             │  Ed25519 keypair → NodeID       │
│  Transport                 │  HTTP (P1) → Iroh/QUIC (P2)     │
│  Sync Protocol             │  XMesh-Sync v1 → Loro CRDT (P3) │
│  Data Commons              │  Opt-in telemetry + rewards     │
└────────────────────────────┴─────────────────────────────────┘
┌──────────────────────────────────────────────────────────────┐
│                     Governance (Bicameral DAO)                │
├─────────────────────┬────────────────────────────────────────┤
│  Chamber 1 (Users)  │  50% weight, 1 wallet = 1 vote        │
│  Chamber 2 (Council)│  50% weight, public members            │
│  Veto               │  66% council for security              │
│  Overrule           │  75% community can override veto       │
│  XP Rewards         │  Earned for sharing usage data         │
└─────────────────────┴────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────┐
│                 Runtime Health & Auto-Improvement             │
├──────────────────────────────────────────────────────────────┤
│  Health Checks     │  Disk, DB, Mesh peers, Embeddings       │
│  Benchmarks        │  Recall@k, latency, coverage            │
│  Gap Analysis      │  Compare results to 100% target         │
│  Auto-Fix          │  Generate experiments → measure → merge │
└──────────────────────────────────────────────────────────────┘
```

## CRITICAL DECISIONS

| Date       | Decision                                   | Context                                   |
|------------|--------------------------------------------|-------------------------------------------|
| 2026-03-05 | Monolithic Rust binary via `adk-rust`      | Maximizes Tokio parallelism & performance while remaining LLM-agnostic. |
| 2026-03-05 | Multi-Layer System 3 RAG Architecture      | Emulates human rational thought checking to eliminate standard LLM hallucinations. |
| 2026-03-10 | Rebranded to Xavier                        | Transitioning to a production-ready cognitive memory system for OpenClaw. |
| 2026-03-11 | Agentic-first memory substrate             | Xavier must support agent workflows with embedded RAG and RAG flows with escalated agentic reasoning. |
| 2026-03-29 | Local-first LLM provider defaults          | `ModelProviderKind::Local` checked first; Ollama (localhost:11434) default. |
| 2026-04-14 | Mesh P2P (Ed25519)                         | P2P sync with Ed25519 identity, Data Commons for anonymous telemetry. |
| 2026-06-10 | Bicameral DAO governance                   | 50% users + 50% council, 7-day activity requirement for voting. |
| 2026-06-10 | XP Tokenomics                              | Node wallet, rewards for contribution, stake/unstake, 5% fee burn. |
| 2026-06-16 | Runtime health + auto-improvement          | Self-monitoring loop → benchmark → gap → experiment → fix. |
| 2026-06-16 | Dual License (MIT + Mesh License)          | MIT for standalone use; Mesh License activates governance + data commons opt-in. |

## Source Map

```
src/
├── main.rs                 # Entry point, init_logger, Cli dispatch
├── cli/                    # CLI commands (server, recall, add, mesh, token)
├── workspace/              # WorkspaceState, WorkspaceConfig, runtime init
├── memory/                 # QmdMemory, VecSqliteMemoryStore, entity_graph
├── mesh/                   # P2P network, governance, tokenomics, protocol
│   ├── node.rs             # NodeIdentity (Ed25519)
│   ├── protocol.rs         # XMesh-Sync v1 types
│   ├── governance.rs       # Mesh DAO mock
│   ├── tokenomics/         # Wallet, Rewards, Transaction
│   ├── data_consent.rs     # Opt-in consent manager
│   ├── data_sanitizer.rs   # Anonymization layer
│   ├── acl.rs              # Per-node ACL
│   ├── crypto_gating.rs    # Temporal access gating
│   └── telemetry_collector.rs # Aggregated telemetry
├── data_commons/           # Governance, reputation, training funnel
│   ├── governance.rs       # Bicameral DAO implementation
│   ├── reputation.rs       # EigenTrust-based reputation
│   └── funnel.rs           # Data contribution pipeline
├── security/               # Auth, encryption, sessions
├── observability/          # Logging, tracing, health
├── embedding/              # Cloud + local embedding providers
└── storage/                # SQLite + sqlite-vec
```

## Key Architectural Constraints

1. **Monolithic binary** — single `xavier` binary with feature gates for mesh, telegram, gllm
2. **Local-first** — all features work offline; mesh is optional opt-in
3. **Encrypted at rest** — AES-256-GCM + Argon2 for stored memory
4. **Data sovereignty** — data never leaves node without explicit consent
5. **Dual license** — MIT (standalone) + Mesh License (governance opt-in)
6. **Runtime health** — continuous self-monitoring and auto-improvement loop
