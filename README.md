# Xavier — Fast Vector Memory & Communal Context Runtime for AI Agents

> Some features are experimental or beta — see [Experimental and beta features](#-experimental-and-beta-features).

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](https://www.gnu.org/licenses/agpl-3.0.html)
[![Version](https://img.shields.io/badge/version-0.2.17-brightgreen.svg)](https://github.com/iberi22/xavier)
[![CI Build Status](https://github.com/iberi22/xavier/actions/workflows/ci.yml/badge.svg)](https://github.com/iberi22/xavier/actions/workflows/ci.yml)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)

Xavier is a **high-performance, Rust-based vector memory runtime for AI agents** with native HTTP REST, CLI, and Model Context Protocol (MCP) entry points. It manages vector embeddings, hierarchical context graphs, and semantic relationships using a robust SQLite-backed store (`sqlite-vec`), granting agents sub-millisecond contextual recall without external service dependencies.

---

## 🚀 Quick Start

### 1. Build & Installation

Ensure you have Rust (1.94+) installed.

```bash
# Clone repository
git clone https://github.com/iberi22/xavier.git
cd xavier

# Build release binary
cargo build --release

# Or install binary locally to PATH
cargo install --path . --locked
```

For platform-specific installation options (such as Windows Scheduled Tasks or Linux systemd setup), see [Installer Guides](docs/guides/WINDOWS_INSTALL.md).

### 2. Environment Setup

Configure master credentials and target workspace path:

```bash
export XAVIER_TOKEN=your-secure-token
export XAVIER_WORKSPACE_DIR=.
```

### 3. Launching Services

Xavier provides unified daemons for HTTP REST and MCP interfaces:

```bash
# Start HTTP REST server on port 8006 and MCP HTTP+SSE on port 8100
xavier http

# Start HTTP REST server only (disabling MCP HTTP port)
xavier http --mcp-port 0

# Start MCP stdio server for Cursor, Claude Desktop, or CLI integration
xavier mcp
```

### 4. Basic CLI Usage

```bash
# Add a memory fragment
xavier add "Architecture decision: Use SQLite-vec for local vector storage" "ADR-001" --kind decision
```

---

## 🧠 Canonical Agent Skills

Xavier provides native, standardized agent skills located in [`skills/`](skills/) for continuous development, code-graph navigation, and maintenance:

- **[`xavier-cognitive-memory`](skills/xavier-cognitive-memory/SKILL.md)**: Memory persistence, hybrid search (BM25 + vector), and MCP protocol integration.
- **[`xavier-rtk-execution`](skills/xavier-rtk-execution/SKILL.md)**: High-speed command execution and 60–90% token reduction via the `rtk-kernel` proxy plugin.
- **[`xavier-code-graph-analysis`](skills/xavier-code-graph-analysis/SKILL.md)**: AST symbol navigation, call hierarchy mapping, and blast-radius analysis.
- **[`xavier-wave-verification`](skills/xavier-wave-verification/SKILL.md)**: Automated GitCore feature verification and CI test execution.
- **[`xavier-maintenance-hygiene`](skills/xavier-maintenance-hygiene/SKILL.md)**: Anti-sprawl guidelines, database cache quarantine, and deprecated script handling.

```bash
# Search memory fragments
xavier search "SQLite vector storage" -n 5

# Recall memories with score breakdown
xavier recall "vector memory" --limit 5

# Diagnose local node health and runtime readiness
xavier doctor
```

### 5. Preflight Verification

Before committing changes or triggering releases, verify manifest version sync and readiness using the SWAL preflight runner:

```bash
# Run preflight check using local repo path
node ~/proyectosSWAL/periferia/swal-preflight/bin/swal-preflight.js check --cwd .

# Recommended shell alias
alias swal-preflight="node $HOME/proyectosSWAL/periferia/swal-preflight/bin/swal-preflight.js"
swal-preflight check --cwd .

# npm i -g @swal/preflight # when published
```

---

## 📦 Downloads

Latest release: **[v0.2.17](https://github.com/iberi22/xavier/releases/tag/v0.2.17)** — built via `.github/workflows/release.yml` (3 targets + SHA256, plus the `XavierSetup.exe` Windows installer).

| OS | Arch | Artifact |
|---|---|---|
| Linux | x86_64 | `xavier-v0.2.17-x86_64-unknown-linux-gnu.tar.gz` |
| macOS | aarch64 (Apple Silicon) | `xavier-v0.2.17-aarch64-apple-darwin.tar.gz` |
| Windows | x86_64 | `xavier-v0.2.17-x86_64-pc-windows-msvc.zip` |

Each artifact has a `.sha256` sidecar.

**Linux desktop app:** download `Xavier_<version>_amd64.AppImage` from the release, `chmod +x`, double-click (or a `.deb`). See [docs/desktop.md](docs/desktop.md).

```bash
# Linux example
curl -L https://github.com/iberi22/xavier/releases/download/v0.2.17/xavier-v0.2.17-x86_64-unknown-linux-gnu.tar.gz -o xavier.tar.gz
curl -L https://github.com/iberi22/xavier/releases/download/v0.2.17/xavier-v0.2.17-x86_64-unknown-linux-gnu.tar.gz.sha256 -o xavier.sha256
sha256sum -c xavier.sha256
tar -xzf xavier.tar.gz && ./xavier --help
```

Docker image: `ghcr.io/iberi22/xavier:0.2.17` (also `:latest`) — also see [Downloads page](docs/site/src/content/docs/downloads.mdx) (Starlight).

---

## 🐳 Docker

### Production (with local Ollama)

```bash
# 1. Set token
cp .env.example .env  # edit XAVIER_TOKEN
# 2. Up
docker compose up -d
# Xavier on http://localhost:8006, Ollama via host.docker.internal:11434
```

Healthcheck: `curl -fsS http://localhost:8006/ready` (503 until the memory store and code graph are ready). `GET /health` always answers HTTP 200 and reports `status` / `degraded_reasons` in the body, so it is not a liveness signal; `xavier health` exits non-zero when the node is `unhealthy`.

### Development mode (no token, browser panel)

```bash
XAVIER_TOKEN=dummy-dev-token docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d
# or
XAVIER_TOKEN=dummy docker compose up -d
```

`docker-compose.yml` sets up local development. Ensure you use `XAVIER_TOKEN` for authentication.

---

## 🖥️ UI Modes

| Mode | How to run | URL | Notes |
|---|---|---|---|
| **Browser (prod)** | `xavier http` (serves `panel-ui/build`) | `http://localhost:8006/` and `/panel` | No Tauri, uses `VITE_XAVIER_API_TOKEN` + polling fallback |
| **Browser (dev)** | `pnpm --filter xavier-panel-ui dev` + `cargo run -- http` | `http://localhost:5173` (Vite) proxied to `:8006` | Hot-reload, use `XAVIER_TOKEN` |
| **Tauri Desktop** | `pnpm --filter xavier-panel-ui tauri dev` | Native window | Requires `__TAURI_INTERNALS__` guard + dynamic `invoke`/`listen` |
| **Custom panel path** | `XAVIER_PANEL_UI_DIR=/path/to/dist xavier http` | Same as browser | Priority: `XAVIER_PANEL_UI_DIR` → `<exe_dir>/panel-ui/build` → `<cwd>/panel-ui/build` → `CARGO_MANIFEST_DIR/panel-ui/build` (see `src/server/panel/assets.rs`) |

---

## 🐛 Known Issues

See **[docs/KNOWN_ISSUES.md](docs/KNOWN_ISSUES.md)** for the full 15-item table (browser-compat, config inline-comment pitfall, panel `build` vs `dist` drift, etc.).

Short summary:

- Browser panel required `__TAURI_INTERNALS__` guards + `useApiToken` hook (fixed in WAVE-5, `4af11709`).
- `XAVIER_TOKEN=foo # comment` inline — the `# comment` becomes part of the literal token; put comments on their own lines.
- `XAVIER_PANEL_UI_DIR` priority list only in `assets.rs` — now documented in `docs/reference/ENV_VARS.md`.
- `/v1/auth/login`, `/v1/auth/register`, `/v1/auth/refresh`, `/v1/auth/logout` and `/v1/auth/totp/*` were dead endpoints (backed by a user store nothing ever wrote to) and are now deprecated (`308`/`410` pointing to `/auth/*`) — see [GH #2545](https://github.com/iberi22/xavier/issues/2545) and [`docs/api/README.md`](docs/api/README.md#2-authentication-protocol) for the real, working auth API and a tested install walkthrough in [`docs/manual/GETTING_STARTED.md`](docs/manual/GETTING_STARTED.md#4-instalación-desde-cero-docker--primer-usuario-admin).

---

## 🧪 Experimental and beta features

These features are experimental or beta: APIs and on-disk formats may change between releases. Do not rely on them for production workloads or irreplaceable data, and keep your own backups.

| Feature | Status | What that means today |
|---|---|---|
| Encryption at Rest | beta | Memory records are sealed at rest; some auxiliary stores are still being migrated, and it has not been exercised against long-lived production data (ledger: `feat-encryption-at-rest`). |
| Xavier Mesh P2P Network & Data Commons | beta | P2P memory sync between nodes (ledger: `feat-mesh-network`). |
| SWAL service network (internal telemetry) | beta | Opt-in service telemetry sharing between nodes (ledger: `feat-mesh-service-network`). |
| Private mesh per key wallet | beta | Private mesh scoped to nodes of one key wallet (ledger: `feat-mesh-private-wallet`). |
| Vault hardening (anti-exfiltration, MCP secret tools, leases dashboard) | beta | Secret lending, anti-exfiltration limits and MCP secret tools work; the leases dashboard is not wired to a route yet (ledger: `feat-vault-hardening`). |
| CLI health/stats parity (`xavier health` / `stats`) | beta | CLI output tracks `GET /health` and MCP stats; exits non-zero when unhealthy (ledger: `feat-cli-health-stats-parity`). |
| Code-graph dump path handling | beta | Dump honours its path argument and working directory (ledger: `feat-codegraph-dump-path`). |
| Bounded `xavier doctor` | beta | Probes run under timeouts and the report always prints (ledger: `feat-doctor-bounded`). |
| E2E observability contract | beta | MCP health agrees with `GET /health` in a hermetic gate (ledger: `feat-e2e-observability-contract`). |
| Guardian truth (`env_status` / `log_scan`) | beta | Reports measured host/log state (ledger: `feat-guardian-truth`). |
| Health truth | beta | Unmeasured coverage reports `unknown`, not healthy (ledger: `feat-health-truth`). |
| Bounded `xavier improve` | beta | Per-stage deadlines with partial-progress persistence (ledger: `feat-improve-bounded`). |
| MCP health honesty (`sys_health` / `health_check`) | beta | Reports measured values only; unmeasured stays unset (ledger: `feat-mcp-health-honesty`). |
| Memory write integrity | beta | Writes reject corrupt content; retries are idempotent per path (ledger: `feat-memory-write-integrity`). |
| Skill dispatch confidence floor | beta | Minimum dispatch confidence with ingest validation (ledger: `feat-skill-dispatch-confidence`). |
| Per-repo memory instance (XAV-REPO) | beta | Per-repository memory instance with its own config and privacy level (ledger: `feat-repo-memory-instance`). |
| Code-graph fingerprint (`ksm-sfp/1`) | in progress | Per-function structural fingerprints for a vulnerability atlas (ledger: `feat-code-graph-fingerprint`). |
| OSS Scanner enrolment files | in progress | Build/scan configuration only, no runtime behavior (ledger: `chore-oss-scanner-enrollment`). |
| Autonomous Delivery Pipeline | planned | Evidence-gated ship/show/ask delivery (ledger: `feat-autonomous-delivery`). |
| Generative UI (OpenUI) | planned | Structured answers rendered as OpenUI markup (ledger: `feat-generative-ui`). |
| Skill store / registry / projector / ephemeral / drift-audit / usage-telemetry | planned | Skill subsystem redesign, grouped (ledger: `feat-skill-*`: store-config, registry-unify, projector, ephemeral, drift-audit, usage-telemetry). |
| `gpud` router (`/v1/gpud/*`) | design only | Defined in `src/server/gpud_sidecar.rs` but never mounted; paths are unreachable. |
| `telecom_*` MCP tools | preview | Tool names are registered, but dispatch has no backend wired and returns `telecom backend unavailable` (`src/server/mcp/telecom_tools.rs`, `src/server/mcp/server.rs`). |
| Maloca model router (`/v1/maloca/models/*`) | placeholder | Handlers return `501 Not Implemented` (`src/server/maloca/model_routes.rs`). |
| Maloca alignment routes (`/v1/maloca/alignment*`) | preview | Returns a static checklist and score (`src/server/maloca/alignment_route.rs`). |
| Mesh tokenomics / wallet | placeholder | Module header describes a placeholder XP system (`src/mesh/tokenomics/mod.rs`). |
| Mesh governance / DAO | preview | Off-chain proposal store with fixed example entries; on-chain client is behind the optional `dao-evm` build feature (`src/mesh/governance/`, `Cargo.toml`). |
| Mesh transports (iroh / libp2p) | preview | `mesh` (iroh) is an optional build feature; `libp2p` is an empty feature and the code notes the transport as a deprecated stub (`Cargo.toml`, `src/mesh/maturity.rs`). |
| Mesh maturity in `/health` | preview | Reports 0/100 projections of checked flags and starts unmeasured (`src/mesh/maturity.rs`, `src/mesh/dashboard.rs`). |

---

## 🏗️ Architecture Overview

Xavier is organized into modular subsystems designed for autonomous agent contextual awareness and local-first execution:

```
src/
├── memory/        — Hybrid RAG Engine (SQLite-vec + BM25 + Semantic Search + Belief Graph)
├── codebase/      — Code GraphDB (Tree-sitter AST symbol extraction & call dependency graphs)
├── mesh/          — P2P Mesh Network (QUIC/Iroh transport, offline SQLite queue, keychains)
├── agents/        — Agent lifecycle, Provider Router (Ollama / Cloud LLM switching), Rate limits
├── server/        — Axum HTTP REST server, MCP Server (HTTP+SSE & Stdio), GPU sidecar
├── maloca/        — Maloca presentation bridge, HumanChallenge engine, Backlog & App Registry
└── storage/       — Centralized SQLite connection management & PRAGMA configuration
```

- **`memory`**: Provides hybrid vector + keyword search (`BM25` + `sqlite-vec`), hierarchical context tree clustering (HCE engine), and dynamic zone weighting (1.5x boost for active work contexts). Memory symbol links are resolved on-demand.
- **`codebase` / `code-graph`**: Performs incremental language parsing (Rust, TypeScript, Python, Go, Java, C/C++) to compute call chains, reverse dependencies, complexity hotspots, and blast radius for code entities.
- **`mesh`**: Handles decentralized P2P synchronization between same-tenant nodes with last-write-wins timestamp conflict resolution and local SQLite offline queue fallback (`offline_queue` table).
- **`agents` & `server`**: Houses the Axum HTTP REST API, MCP JSON-RPC transports (Stdio and SSE), and provider routing.
- **`maloca`**: Connects Xavier to the Maloca presentation portal, tracking ecosystem alignment, backlog features, agent challenge scoring, and app registry metadata.

---

## 📚 API & Interface Reference

### 🌐 HTTP REST API (`:8006`)

All authenticated HTTP routes require the `X-Xavier-Token` or `Authorization: Bearer <token>` header.

#### Maloca V1 Services (`/v1/maloca/*`)
| Endpoint | Method | Description |
|---|---|---|
| `/v1/maloca/registry` | `GET` | List ecosystem applications (supports ETag `If-None-Match`) |
| `/v1/maloca/registry/{app_id}` | `GET` | Retrieve specific application metadata entry |
| `/v1/maloca/alignment` | `GET` | Retrieve ecosystem alignment audit score and active flags |
| `/v1/maloca/alignment/goals` | `GET` | Retrieve canonical SWAL goals and verification criteria |
| `/v1/maloca/backlog/unified` | `GET` | Query aggregated multi-repo features (`wave`, `status`, `priority`) |
| `/v1/maloca/backlog/summary` | `GET` | Retrieve backlog progress metrics (30s TTL cache) |
| `/v1/maloca/models/infer` | `POST` | Execute inference request via model router (placeholder, returns 501 Not Implemented) |
| `/v1/maloca/models/list` | `GET` | List available Ollama and cloud LLM models (placeholder, returns 501 Not Implemented) |
| `/v1/maloca/models/health` | `GET` | Query model engine health status (placeholder, returns 501 Not Implemented) |
| `/v1/maloca/challenges/generate` | `POST` | Generate cognitive HumanChallenge candidate |
| `/v1/maloca/challenges/answer` | `POST` | Submit response to challenge for semantic similarity scoring |
| `/v1/maloca/challenges/list` | `GET` | List active challenges |
| `/v1/maloca/challenges/stats` | `GET` | Retrieve challenge engine statistics |

#### Not mounted (design only)

The `gpud` router is never merged into the server, so these paths are unreachable today.

#### GPU Discovery & Sidecar (`/v1/gpud/*`)
| Endpoint | Method | Description |
|---|---|---|
| `/v1/gpud/health` | `GET` | GPU sidecar health check |
| `/v1/gpud/detect` | `GET` | Hardware acceleration probe (NVIDIA nvcc/nvidia-smi, ROCm, Apple Silicon) |
| `/v1/gpud/serve` | `POST` | Request GPU compute allocation |
| `/v1/gpud/status` | `GET` | Monitor current VRAM allocation and fallback CPU state |

#### Maintenance & Memory (`/v1/maintenance/*`, `/memory/*`)
| Endpoint | Method | Description |
|---|---|---|
| `/v1/maintenance/reindex-embeddings` | `GET` | Reindex missing vector embeddings (batched with 409 Conflict anti-double guard) |
| `/memory/search` | `POST` | Hybrid memory vector search query |
| `/memory/add` | `POST` | Add new memory fragment |

---

### 🔌 Model Context Protocol (MCP) Tools

Xavier exposes standard MCP tools for integration with Cursor, Claude Desktop, and autonomous agent loops:

#### Core System Tools
- `health_check`: Report full system health status (database, embedding, mesh).
- `sys_health`: Host node guardian metrics (PSI, swap usage, load averages, top processes).
- `log_scan`: Scan runtime logs under `~/.xavier/logs` with regex secret redaction.
- `env_status`: Check systemd services, TCP network connectivity, and swap memory.
- `ticket_create`: Create GitHub issues or Maloca backlog entries with deduplication.
- `get_code_graph`: Export portable code graph dump (`.xavier/codegraph.json`).
- `codegraph_explore`: Search code graph symbols by name or query.
- `trace_path`: Trace forward dependencies or caller chains for code symbols.

#### Memory Management Tools
- `mem_search`: Progressive disclosure search returning candidate headers, snippets, and provenance (without full body by default).
- `mem_context` / `memory_context`: Page-in full or partial memory content bounded by `max_chars` and per-doc limits.
- `create_memory`: Store structured memory documents with path, content, kind, and provenance.
- `save_fragment` (`memoryfragment_save`): Save atomic memory fragments tagged with importance and agent context.
- `memory_save`: Store free-form text memories with optional metadata and namespace.
- `get_memory` / `memoryfragment_get`: Retrieve specific memory documents or fragments by ID.
- `memory_prune`: Prune stale or duplicate memories by kind, age, or path prefix (supports `dry_run`).

#### Context & Issue Tools
- `xavier_context_save`: Create session context checkpoints.
- `xavier_context_restore`: Restore optimized context blocks tailored to specified token budget.
- `xavier_context_search`: Search within saved session contexts.
- `xavier_token_savings`: Report token savings statistics achieved via context compression.
- `xavier_issue_context_package`: Produce a `PreciseChange` context package for a GitHub issue.

---

## 📄 PageIndex: tree-based document retrieval

Index a long document (PDF, markdown, text, legal) as a table-of-contents tree and let the calling agent navigate it with `pageindex_*` MCP tools (`browse_documents`, `get_document`, `get_document_structure`, `get_page_content`, `search`, `index_document`) or `/v1/pageindex/*`. No chunking, no embeddings, no LLM inside Xavier; answers carry node ids and page ranges. On the public PageIndex-OSS-Benchmark (62 questions, 34 PDFs) an agent reached 60/62 reading 1.7 pages per question. It complements BM25/vector search rather than replacing it. See [docs/features/pageindex.md](docs/features/pageindex.md).

---

## 🧠 Agent Skill: Cognitive Memory & CodeGraph

For autonomous coding assistants and agents (**Antigravity**, **Hermes**, **Jules**, **OpenClaw**, **Cursor**), Xavier ships with a ready-to-use canonical skill: [`skills/xavier-cognitive-memory/SKILL.md`](skills/xavier-cognitive-memory/SKILL.md).

### Post-Installation Setup for Agents

Install the skill into your global agent configuration:

```bash
# For Antigravity / Gemini agents
mkdir -p ~/.gemini/config/skills/xavier-cognitive-memory
cp skills/xavier-cognitive-memory/SKILL.md ~/.gemini/config/skills/xavier-cognitive-memory/SKILL.md

# For Hermes agents
mkdir -p ~/.hermes/skills/xavier-cognitive-memory
cp skills/xavier-cognitive-memory/SKILL.md ~/.hermes/skills/xavier-cognitive-memory/SKILL.md
```

### Mandatory Agent Cycle (PRE → EXEC → POST)

1. **PRE (Context Discovery):**
   ```bash
   xavier search "<task or feature keyword>" -n 5
   ```
2. **EXEC (Code-Graph Navigation & Blast Radius):**
   ```bash
   xavier code find <SymbolName>
   xavier code blast-radius <SymbolName>
   ```
3. **POST (Persist Findings):**
   ```bash
   xavier add "Implemented X with decision Y" --kind decision --cluster "<project>"
   ```

---

### 💻 CLI Command Summary

```bash
xavier http [PORT] [--mcp-port MCP_PORT]         # Start HTTP REST and MCP SSE servers (port is positional)
xavier mcp                                        # Start stdio MCP JSON-RPC transport
xavier search <QUERY> [-n MAX]                    # Search memory store
xavier recall <QUERY> [--limit N]                 # Recall memories with score details
xavier add <CONTENT> [TITLE]                      # Add memory fragment
xavier chat [--prompt P] [--agent A]              # Interactive or single-shot agent chat
xavier ask <PROMPT>                               # Quick question alias for chat
xavier code scan <PATH>                           # Index code directory into code graph
xavier code find <QUERY>                          # Find symbols in code graph
xavier code blast-radius <SYMBOL>                 # Calculate call graph blast radius
xavier nav affected --path <PATH>                 # Impact analysis for file/concept change
xavier mesh list                                  # List known P2P mesh peers
xavier node create                                # Initialize SWAL node identity
xavier nodes add --provider <PROVIDER>            # Provision BYO node instance
xavier secrets lend <SECRET> <AGENT>              # Issue ephemeral secret lease
xavier doctor [--format table|json|markdown]      # Diagnostic node checks
xavier improve --ci                               # Run auto-improvement benchmark cycle
xavier regen benchmark                            # Measure recall@k metrics
```

---

## ⚙️ System Configuration Reference

### Environment Variables

| Variable | Default | Description |
|---|---|---|
| `XAVIER_TOKEN` | *Required* | Master authentication token for HTTP REST API requests. **Note:** In `.env` files, place comments on their own lines — inline comments (e.g. `XAVIER_TOKEN=foo # comment`) become part of the literal token. Inspect active process environment via `tr '\0' '\n' < /proc/$(pgrep xavier)/environ | grep XAVIER_TOKEN`. |
| `XAVIER_PORT` | `8006` | Main HTTP REST server bind port |
| `XAVIER_HOST` | `0.0.0.0` | Network bind interface for HTTP server |
| `XAVIER_WORKSPACE_DIR` | `.` | Root directory path for indexing and local database operations |
| `XAVIER_EMBEDDING_CACHE_ENABLED` | `true` | Enables persistent SQLite LRU cache for vector embeddings |
| `XAVIER_EMBEDDING_CACHE_CAPACITY` | `10000` | Maximum in-memory LRU cache capacity |
| `XAVIER_EMBEDDING_CACHE_TTL` | `24` | Embedding cache TTL in hours |
| `XAVIER_EMAIL_DEDUP_SECS` | `300` | Rate-limiting window in seconds for email notification deduplication |
| `XAVIER_MESH_AUTO_REPAIR` | `1` | Auto-repair peer connection manager (`0` or `false` disables auto-reconnect) |

### Centralized SQLite PRAGMAs

Xavier centralizes database PRAGMA configuration across all SQLite storage connections:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -8000;      -- ~8MB memory cache
PRAGMA mmap_size = 268435456;   -- 256MB memory-mapped I/O
PRAGMA temp_store = MEMORY;
PRAGMA busy_timeout = 5000;     -- 5 second timeout on locks
PRAGMA foreign_keys = ON;
```

---

## 🛖 The Maloca RAG & Presentation Portal

**Maloca** is Xavier's human-in-the-loop presentation portal (named after the Amazonian communal house). It presents system diagrams, release chronicles (ADRs), module breakdowns, and code diff statistics.

```
┌────────────────────────┐       ┌────────────────────────┐
│  Git Commit History    │ ────> │  Chronicle Harvester   │
└────────────────────────┘       └───────────┬────────────┘
            ▲                                │
            │                                ▼
┌───────────┴────────────┐       ┌────────────────────────┐
│   Maloca Presentation  │ <──── │   Auto-Docs & RAG      │
│   (public/maloca/)     │       └────────────────────────┘
└────────────────────────┘
```

1. **Pre-commit Chronicle**: Commits, code symbols, and git diff statistics are harvested automatically during pre-commit workflows.
2. **Context Memory**: Module understandings and development conversations are indexed into Xavier's vector store.
3. **Interactive Dashboard**: The panel web UI under `panel-ui/` renders Maloca tabs (Overview, Registry, Goals, Backlog, Challenges, Models).

---

## 📂 Documentation Manifest

- [Agent Rules (AGENTS.md)](AGENTS.md) — Mandatory guidelines for memory formatting and agent behavior.
- [Agent Integration Guide](docs/guides/agent-integration.md) — Connect autonomous agents (Hermes, Gestalt, Jules) to Xavier.
- [Feature Status](docs/FEATURE_STATUS.md) — Comprehensive surface verification checklist.
- [CLI Reference Guide](docs/guides/CLI_REFERENCE.md) — Extended CLI command arguments and usage examples.
- [MCP Integration Guide](docs/guides/MCP_INTEGRATION.md) — Cursor and Claude Desktop MCP setup guide.
- [Quickstart Guide](docs/guides/QUICKSTART.md) — Quick start walkthrough.
- [System Architecture](docs/ARCHITECTURE.md) — Domain layout and hexagonal design documentation.

---

## 🦀 Minimum Supported Rust Version (MSRV)

Xavier guarantees compilation on Rust **1.94** or higher.

- **Current Pin**: Defined via `rust-version = "1.94"` in `Cargo.toml`.
- **Bump Policy**: MSRV bumps follow a `stable-minus-2` policy (never exceeding `stable - 2` releases at the time of bump) when required by upstream dependency updates. Any MSRV bump is explicitly announced in a patch release.
- **Scope**: Applies to standard crate library and binary builds. Development tooling, UI runtimes, or optional feature sidecars may require newer toolchain versions.

---

## 🙏 Acknowledgements

- [PageIndex](https://github.com/VectifyAI/PageIndex) by Vectify AI (MIT, © 2025 Vectify AI) inspired Xavier's tree-based document retrieval (idea and tool design; reimplemented from scratch in Rust, no upstream code included), and its [OSS benchmark](https://github.com/VectifyAI/PageIndex-OSS-Benchmark) was used to evaluate it. Details in [docs/features/pageindex.md](docs/features/pageindex.md).

---

## 🛡️ License

AGPL-3.0-only — see [LICENSE](LICENSE) for details. Cognitive memory runtime for autonomous agents within SouthWest AI Labs (SWAL).
