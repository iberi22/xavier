# SRC.md - Source Code Reference

**Project:** Xavier
**Version:** 0.12.0
**Generated:** 2026-07-21
**Repository:** https://github.com/iberi22/xavier
**Location:** `.gitcore/` (per GitCore Protocol)

## Directory Structure

```
Xavier/
├── .gitcore/        # Agent documentation (per GitCore Protocol)
├── benches          # Rust benchmarks (Criterion)
├── benchmark-results # Storage for benchmark execution outputs
├── benchmarks       # Benchmark scripts and configurations
├── benchmark_test_results # Results from automated benchmark suites
├── bin              # Auxiliary binaries and compiled tools
├── code-graph       # AST/symbol indexing sidecar (Workspace member)
├── data             # Local databases and persistent state (Runtime)
├── docker           # Dockerfiles and compose configurations
├── docs             # Technical documentation, ADRs, and guides
├── panel-ui         # Frontend dashboard (React/Vite)
├── scripts          # Maintenance and automation scripts
├── skills           # Agent capabilities and skill modules
├── src              # Core Rust source code
├── target           # Rust build artifacts
├── tests            # Integration and E2E tests
└── web              # Web dashboard components
```

## Root Modules

| Directory | Status | Purpose |
|-----------|--------|---------|
| `benches/` | Complete | Criterion benchmarks for embeddings, search, and retrieval |
| `code-graph/` | Complete | AST/symbol indexing sidecar crate (libclang-based) |
| `docs/` | Complete | ADRs, architecture specs, deployment guides |
| `panel-ui/` | Complete | React 19 + Vite + TypeScript frontend dashboard |
| `scripts/` | Complete | Automation scripts (install, setup, CI helpers) |
| `src/` | Complete | Core Xavier engine — cognitive memory system |
| `tests/` | Complete | Integration + E2E tests (HTTP API, HORMER, auth, mesh) |

## Core Submodules (src/)

### Architecture Overview

Xavier follows **hexagonal (ports & adapters) architecture** with 3 cognitive layers:

- **System 1** — Reflexive search/retrieval (BM25 + Vector hybrid)
- **System 2** — Deliberative reasoning with HORMER navigation policy
- **System 3** — Oversight, governance, and safety monitoring

### Module Reference

#### `src/a2a` — Agent-to-Agent Communication
Agent message protocol, routing, and inter-agent data exchange. Key types: `AgentMessage`, `MessageRouter`.

#### `src/adapters` — Hexagonal Adapters
**Inbound** (HTTP, MCP, CLI) and **Outbound** (SQLite, Vector Store, Embedding providers) ports.

#### `src/agents` — Cognitive Agent Runtime
Runtime for System 1/2/3 agents, provider-agnostic LLM routing.
- **Key types:** `AgentRuntime`, `SystemAgent`, `RateLimitManager`, `ProviderRouter`
- **HORMER** (`agents/hormer/`): Learned navigation policy with simplified GRPO optimization

#### `src/api` — Internal API Definitions
Request/response types for skills, search, graph, settings, and timeline APIs.

#### `src/app` — Application Use Cases
Orchestration layer: memory adapter, proxy, security service, context management.

#### `src/billing` — Usage & Billing
Stripe integration, usage tracking, and quota management.

#### `src/checkpoint` — Session Checkpointing
Snapshot and restore agent session state for crash recovery.

#### `src/chronicle` — Documentation Harvester
Auto-generates documentation from code changes via git hooks.

#### `src/cli` — CLI Implementation
Command handlers (`handlers/`), state management, and TUI components.
- **Key handlers:** `navigation.rs` (ls/cd/pwd), `mesh.rs`, `license.rs`, `governance.rs`

#### `src/codebase` — Codebase Analysis
Local repository scanning, connection management, file indexer.

#### `src/consistency` — Memory Regularization
Consistency checks, deduplication, and integrity validation for stored memories.

#### `src/consolidation` — Memory Consolidation
Merging, reflection, and long-term memory stabilization.

#### `src/context` — Context Management
Skill dispatching, orchestrator, and context pack export/import.

#### `src/coordination` — Agent Coordination
Event bus (`XavierEventBus`), agent registry (`SimpleAgentRegistry`), and key lending.

#### `src/crypto` — Cryptography
Encryption primitives (AES-256-GCM), key derivation (Argon2), hashing (BLAKE2).

#### `src/data_commons` — Data Governance
Governance DAO, reputation system, and data tokenization.

#### `src/domain` — Domain Models
Core entities: `BeliefEdge`, `Agent`, `MemoryRecord`, security models.

#### `src/embedding` — Vector Embeddings
Provider abstraction (OpenAI, GLLM, Noop), caching, and batch processing.

#### `src/enterprise` — Enterprise Features
RBAC, multi-tenancy, audit logging, and compliance reporting.

#### `src/memory` — Hierarchical Memory
**QMD** (Quantum Memory Design): Working, Episodic, Semantic layers with SQLite + sqlite-vec.
- **Key types:** `QmdMemory`, `VecSqliteMemoryStore`, `NavTelemetry`, `FileIndexer`, `AgentIndexer`
- Submodules: `qmd/`, `sqlite_vec_store/`, `file_indexer/`, `telemetry/`, `graph_traversal/`

#### `src/mesh` — P2P Networking
Ed25519 identity, peer discovery, memory sync, challenge/namespace/pro_gate (login F1). Mesh = data plane (ledger = Polygon).

#### `src/node_identity` — Decentralized Login (F0 / F3)
BIP39-24, Shamir 2-of-3, vault Argon2id+AES-GCM, check-codes, derive Ed25519+ML-DSA commitment, `hybrid_pack`.
- **CLI:** `xavier node create|recover|status|anchor|anchor-pack`
- **Feature:** `feat-decentralized-login` **95%** · `.gitcore/docs/DECENTRALIZED_LOGIN_PROGRESS.md`

#### `src/polygon_anchor` — Polygon Hash Anchors (F2)
ABI registry, dry-run default, live-prepared, broadcast behind `dao-evm`. Deploy contract = ops.
- **Env:** `SWAL_POLYGON_*`, `SWAL_ANCHOR_*` · Docs: `docs/POLYGON_ANCHORS.md`

#### `src/observability` — Monitoring & Logging
Prometheus metrics, structured tracing (tracing-subscriber), health checks, UsageCounters.

#### `src/retrieval` — Retrieval Strategies
Navigation policy, adaptive zone gating, scoring, and graph traversal.
- **Key types:** `NavigationPolicy`, `NavigationScore`, `TraversalWeights`, `AdaptiveZoneBooster`

#### `src/scheduler` — Background Jobs
Cron-based job scheduling for maintenance, sync, and cleanup tasks.

#### `src/search` — Hybrid Search
BM25 (FTS5) + Vector (sqlite-vec) search with Reciprocal Rank Fusion.
- **Key types:** `HybridSearchEngine`, `ScoredResult`, `RRFScorer`, `SearchCache`

#### `src/secrets` — Secret Management
Vault-backed (Clavis), local keyring, lending engine for ephemeral credentials.

#### `src/security` — Security & Auth
Prompt injection detection (`PromptGuard`), session management, TOTP (RFC 6238), threat detection.
- **Key types:** `SecurityService`, `SessionManager`, `PromptGuard`, `LicenseManager`
- **Modules:** `license.rs` (MIT/Mesh dual-license enforcement)

#### `src/server` — Server Implementations
HTTP (Axum), MCP (Model Context Protocol), and Headless APIs.
- **Key routes:** `/v1/memory/*`, `/v1/search/*`, `/v1/nav/*`, `/v1/auth/*`, `/v1/admin/*`

#### `src/session` — Session Management
User session creation, token validation, session persistence.

#### `src/settings` — System Configuration
`XavierSettings` — runtime configuration load/save, defaults, serialization.

#### `src/sync` — Data Sync Protocol
Manifest-based sync, chunk transfer, conflict resolution for multi-device setups.

#### `src/tasks` — Background Tasks
Async task queue, in-memory/SQLite store, task lifecycle management.

#### `src/telegram` — Telegram Bot
Inline query handling, notification delivery, command processing.

#### `src/tools` — Internal Tools
Kanban board, GitCore protocol validation, integrity checks.

#### `src/ui` — TUI Dashboard
Terminal UI via ratatui (widgets for memory stats, system health, HORMER telemetry).

#### `src/utils` — Common Utilities
HTTP client helpers, file system operations, crypto utilities, path resolution.

#### `src/verification` — System Verification
Automated integrity checks: feature maturity scan, cargo check, test runner.

#### `src/workspace` — Multi-Tenant Workspaces
Isolated workspace config, state management, plan tiers (Community/Pro/Enterprise).

### Important Types and Traits

| Type / Trait | Location | Description |
|---|---|---|
| `Hormer` | `src/agents/hormer/mod.rs` | GRPO-based navigation policy coordinator |
| `NavigationPolicy` | `src/retrieval/policy.rs` | Learned transition scoring with layer + traversal weights |
| `LayerWeights` | `src/retrieval/policy.rs` | Working, Episodic, Semantic memory layer weights |
| `TraversalWeights` | `src/retrieval/policy.rs` | Graph traversal signals (similarity, confidence, recency) |
| `NavTelemetry` | `src/memory/telemetry.rs` | Node visit tracking, hotspot detection, path aggregation |
| `RewardModel` | `src/agents/hormer/reward.rs` | Reward calculation for policy updates |
| `QmdMemory` | `src/memory/qmd_memory.rs` | Three-layer hierarchical memory store |
| `VecSqliteMemoryStore` | `src/memory/sqlite_vec_store.rs` | SQLite + sqlite-vec vector storage |
| `HybridSearchEngine` | `src/search/mod.rs` | BM25 + Vector fusion search engine |
| `XavierSettings` | `src/settings/mod.rs` | Centralized configuration (loads config/xavier.config.json) |
| `SecurityService` | `src/security/mod.rs` | Auth, session validation, prompt guard |
| `PromptGuard` | `src/security/prompt_guard.rs` | Injection detection and rate limiting |
| `ProviderRouter` | `src/agents/provider/router.rs` | Multi-provider LLM routing (OpenAI, Ollama, Gemini, etc.) |
| `WorkspaceState` | `src/workspace/mod.rs` | Per-workspace runtime: memory, config, agent registry |
| `XavierEventBus` | `src/coordination/event_bus.rs` | In-process pub/sub for agent coordination |
| `AdaptiveZoneBooster` | `src/retrieval/gating.rs` | Boost/penalty scoring based on user feedback |
| `LicenseManager` | `src/security/license.rs` | MIT/Mesh dual-license enforcement |
| `Pathfinder` | `src/memory/graph_traversal.rs` | BFS/DFS graph traversal for impact analysis |

## CLI Reference

```
Xavier - Fast Vector Memory for AI Agents

Usage: xavier [COMMAND]

Commands:
  http          Start Xavier HTTP server
                  Options: [PORT], --mcp-port <PORT>
  mcp           Start Xavier MCP-stdio server
                  Options: --http-port <PORT>
  search        Search memories (hybrid BM25 + vector)
  add           Add a memory
  recall        Recall memories with score-based display
  export-pack   Export structured context pack (.xcp) for LLMs
  stats         Show statistics (memory counts, vector store, embeddings)
  reindex       Re-index all memories missing embeddings
  code          Query Xavier code graph (symbols, definitions, references)
  session-save  Save current session context to Xavier
  spawn         Spawn multiple agents with provider routing
  swarm         Launch parallel agents with JSON swarm configuration
  multi-spawn   Batch spawn agents with provider/model routing
  chronicle     Manage Chronicle documentation harvesting
  secrets       Manage ephemeral secrets (Clavis vault)
  vault         Manage the hardware security vault
  usage         Manage provider usage and rate limits
  billing       Show API usage and account balance
  tasks         List and synchronize Xavier tasks
  verify        Run system verification (cargo check, tests, maturity)
  token         Generate authentication tokens
  quota         Show API quotas and limits for providers
  provider      Manage LLM providers and hot-switching
  setup         Run interactive system detection and setup
  doctor        Diagnose local-first health (Ollama, models, DB, config)
  data-commons  Manage Xavier Data Commons and fine-tuning readiness
  governance    Manage Governance DAO (voting, council, proposals)
  wallet        Manage XP wallet and tokenomics
  session       Manage Xavier sessions (list, show, close)
  mesh          Manage Xavier Mesh P2P connections
  export        Export memories to JSON format
  ls            List memories at current or specified path (navigation)
  cd            Change current working directory in memory (navigation)
  pwd           Show current working directory (navigation)
  nav           Navigation and impact analysis (affected, visualize, telemetry)
  task          Task management (list, run, sync)
  sync          Sync operations (memory, tasks, mesh peers)
  license       Manage Xavier license (MIT / Mesh License)
  memory        Memory management and consolidation
  index-self    Index foundational docs into memory store
  scan          System & security scanning
  maturity      Run feature maturity scan and reporting
  cloud         Manage cloud backends and synchronization
  agent         Manage agent memory and IDE sessions
  health        Show system health status
  improve       Run auto-improvement loop (benchmark → fix → validate)
  regen         Context regeneration: measure recall@k, tune RRF weights
  help          Print this message or help of subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

### Key Subcommand Details

| Command | Purpose | Typical Usage |
|---|---|---|
| `http` | Production HTTP server (Axum, multi-threaded) | `xavier http 8006` |
| `mcp` | MCP stdio server (for IDE/tool integration) | `xavier mcp --http-port 8006` |
| `search` | Full-text + vector hybrid search | `xavier search "query" --limit 10` |
| `recall` | Score-sorted memory recall | `xavier recall "query" --threshold 0.5` |
| `doctor` | Diagnose system health (first-run tool) | `xavier doctor` |
| `verify` | Full system verification | `xavier verify --features` |
| `nav` | Navigation sub-commands | `xavier nav visualize`, `xavier nav telemetry` |
| `license` | License management | `xavier license status` |

## Build & Run

```bash
# Prerequisites
rustup update stable
sudo apt install libclang-dev pkg-config libssl-dev  # Linux
# OR on NixOS: nix-shell -p glib pkg-config

# Build
cargo build --release

# Run tests
cargo test --workspace

# Run specific test suite
cargo test -p xavier --lib agents::hormer
cargo test --test hormer_e2e

# Run lints
cargo clippy --workspace -- -D warnings
cargo fmt --check

# Start server
cargo run --release -- http 8006

# Start with custom config
XAVIER_CONFIG_PATH=./config/prod.json cargo run --release -- http 8006
```

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `XAVIER_TOKEN` | — | Auth token for HTTP API (required for production) |
| `XAVIER_CONFIG_PATH` | `config/xavier.config.json` | Override configuration file path |
| `XAVIER_PORT` | `8006` | Default HTTP server port |
| `XAVIER_CODE_GRAPH_DB_PATH` | `data/code-graph.db` | Code-graph sidecar database path |
| `XAVIER_LOG_DIR` | `~/.xavier/logs` | Log output directory |
| `XAVIER_DATA_DIR` | `~/.xavier/data` | Runtime data directory |

## Entry Points

- `src/main.rs` — Primary CLI binary (`xavier` binary target)
- `src/main_tui.rs` — Interactive TUI dashboard (`xavier-tui` binary target)
- `src/server/http/main.rs` — HTTP server entry point
- `src/server/mcp/main.rs` — MCP server entry point
- `src/bin/cortex.rs` — Specialized cognitive reasoning entry point

---
*Maintained manually per GitCore Protocol v3 — Last updated: 2026-07-28 (feat-decentralized-login 95%)*
