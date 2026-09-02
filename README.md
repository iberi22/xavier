# Xavier — Fast Vector Memory & Communal Context Runtime for AI Agents

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](https://www.gnu.org/licenses/agpl-3.0.html)
[![Version](https://img.shields.io/badge/version-0.1.0-brightgreen.svg)](https://github.com/iberi22/xavier)
[![CI Build Status](https://github.com/iberi22/xavier/actions/workflows/ci.yml/badge.svg)](https://github.com/iberi22/xavier/actions/workflows/ci.yml)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)

Xavier is a **high-performance, Rust-based vector memory runtime for AI agents** with native HTTP REST, CLI, and Model Context Protocol (MCP) entry points. It manages vector embeddings, hierarchical context graphs, and semantic relationships using a robust SQLite-backed store (`sqlite-vec`), granting agents sub-millisecond contextual recall without external service dependencies.

---

## 🚀 Quick Start

### 1. Build & Installation

Ensure you have Rust (1.80+) installed.

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

Latest release: **[v0.0.1](https://github.com/iberi22/xavier/releases/latest)** — built via `.github/workflows/release.yml` (3 targets + SHA256).

| OS | Arch | Artifact |
|---|---|---|
| Linux | x86_64 | `xavier-v0.0.1-x86_64-unknown-linux-gnu.tar.gz` |
| macOS | aarch64 (Apple Silicon) | `xavier-v0.0.1-aarch64-apple-darwin.tar.gz` |
| Windows | x86_64 | `xavier-v0.0.1-x86_64-pc-windows-msvc.zip` |

Each artifact has a `.sha256` sidecar.

```bash
# Linux example
curl -L https://github.com/iberi22/xavier/releases/latest/download/xavier-v0.0.1-x86_64-unknown-linux-gnu.tar.gz -o xavier.tar.gz
curl -L https://github.com/iberi22/xavier/releases/latest/download/xavier-v0.0.1-x86_64-unknown-linux-gnu.tar.gz.sha256 -o xavier.sha256
sha256sum -c xavier.sha256
tar -xzf xavier.tar.gz && ./xavier --help
```

Docker image (pending `ghcr` publish): `ghcr.io/iberi22/xavier:0.0.1` — also see [Downloads page](docs/site/src/content/docs/downloads.mdx) (Starlight).

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

Healthcheck: `curl -fsS http://localhost:8006/health`

### Development mode (no token, browser panel)

```bash
XAVIER_DEV_MODE=true docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d
# or
XAVIER_DEV_MODE=true XAVIER_TOKEN=dummy docker compose up -d
```

`docker-compose.yml` respects `XAVIER_DEV_MODE=${XAVIER_DEV_MODE:-false}` — when `true`, the HTTP `X-Xavier-Token` gate is bypassed for local dev (do NOT use in production).

---

## 🖥️ UI Modes

| Mode | How to run | URL | Notes |
|---|---|---|---|
| **Browser (prod)** | `xavier http` (serves `panel-ui/dist`) | `http://localhost:8006/` and `/panel` | No Tauri, uses `VITE_XAVIER_API_TOKEN` + polling fallback |
| **Browser (dev)** | `pnpm --filter xavier-panel-ui dev` + `cargo run -- http` | `http://localhost:5173` (Vite) proxied to `:8006` | Hot-reload, `XAVIER_DEV_MODE=true` |
| **Tauri Desktop** | `pnpm --filter xavier-panel-ui tauri dev` | Native window | Requires `__TAURI_INTERNALS__` guard + dynamic `invoke`/`listen` |
| **Custom panel path** | `XAVIER_PANEL_UI_DIR=/path/to/dist xavier http` | Same as browser | Priority: `XAVIER_PANEL_UI_DIR` → `<exe_dir>/panel-ui/build` → `<cwd>/panel-ui/build` → `CARGO_MANIFEST_DIR/panel-ui/build` (see `src/server/panel/assets.rs`) |

---

## 🐛 Known Issues

See **[docs/KNOWN_ISSUES.md](docs/KNOWN_ISSUES.md)** for the full 15-item table (browser-compat, config inline-comment pitfall, panel `build` vs `dist` drift, etc.).

Short summary:

- Browser panel required `__TAURI_INTERNALS__` guards + `useApiToken` hook (fixed in WAVE-5, `4af11709`).
- `XAVIER_TOKEN=foo # comment` inline — the `# comment` becomes part of the literal token; put comments on their own lines.
- `XAVIER_PANEL_UI_DIR` priority list only in `assets.rs` — now documented in `docs/reference/ENV_VARS.md`.

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
- **`agents` & `server`**: Houses the Axum HTTP REST API, MCP JSON-RPC transports (Stdio and SSE), hardware VRAM/GPU discovery sidecar (`gpud`), and provider routing.
- **`maloca`**: Connects Xavier to the Maloca presentation portal, tracking ecosystem alignment, backlog features, agent challenge scoring, and app registry metadata.

---

## 📚 API & Interface Reference

### 🌐 HTTP REST API (`:8006`)

All authenticated HTTP routes require the `X-Xavier-Token` header (or `XAVIER_DEV_MODE=true` for local development).

#### Maloca V1 Services (`/v1/maloca/*`)
| Endpoint | Method | Description |
|---|---|---|
| `/v1/maloca/registry` | `GET` | List ecosystem applications (supports ETag `If-None-Match`) |
| `/v1/maloca/registry/{app_id}` | `GET` | Retrieve specific application metadata entry |
| `/v1/maloca/alignment` | `GET` | Retrieve ecosystem alignment audit score and active flags |
| `/v1/maloca/alignment/goals` | `GET` | Retrieve canonical SWAL goals and verification criteria |
| `/v1/maloca/backlog/unified` | `GET` | Query aggregated multi-repo features (`wave`, `status`, `priority`) |
| `/v1/maloca/backlog/summary` | `GET` | Retrieve backlog progress metrics (30s TTL cache) |
| `/v1/maloca/models/infer` | `POST` | Execute inference request via model router |
| `/v1/maloca/models/list` | `GET` | List available Ollama and cloud LLM models |
| `/v1/maloca/models/health` | `GET` | Query model engine health status |
| `/v1/maloca/challenges/generate` | `POST` | Generate cognitive HumanChallenge candidate |
| `/v1/maloca/challenges/answer` | `POST` | Submit response to challenge for semantic similarity scoring |
| `/v1/maloca/challenges/list` | `GET` | List active challenges |
| `/v1/maloca/challenges/stats` | `GET` | Retrieve challenge engine statistics |

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
- `mem_search`: Candidate memory retrieval with similarity scores, snippets, and provenance.
- `mem_context`: Packaged memory context injection bounded by `max_records` and `max_chars`.
- `mem_add`: Store semantic, episodic, or procedural memory records.
- `mem_update`: Update existing memory records by path or ID.
- `mem_delete`: Remove memory records from vector store.

#### Context & Issue Tools
- `xavier_context_save`: Create session context checkpoints.
- `xavier_context_restore`: Restore optimized context blocks tailored to specified token budget.
- `xavier_context_search`: Search within saved session contexts.
- `xavier_token_savings`: Report token savings statistics achieved via context compression.
- `xavier_issue_context_package`: Produce a `PreciseChange` context package for a GitHub issue.

---

### 💻 CLI Command Summary

```bash
xavier http [--port PORT] [--mcp-port MCP_PORT]  # Start HTTP REST and MCP SSE servers
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
| `XAVIER_DEV_MODE` | `false` | Development mode; bypasses HTTP authentication when set to `true` |
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

## 🛡️ License

AGPL-3.0-only — see [LICENSE](LICENSE) for details. Cognitive memory runtime for autonomous agents within SouthWest AI Labs (SWAL).
