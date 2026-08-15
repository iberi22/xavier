# Xavier — Fast Vector Memory & Communal Context Runtime for AI Agents

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](https://www.gnu.org/licenses/agpl-3.0.html)
[![Version](https://img.shields.io/badge/version-0.13.0-brightgreen.svg)](https://github.com/iberi22/xavier)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)

Xavier is a **high-performance, Rust-based vector memory runtime for AI agents** with native HTTP, CLI, and MCP entry points. It manages vector embeddings, hierarchical context graphs, and semantic relationships using a robust SQLite-backed store (`sqlite-vec`), granting agents sub-millisecond contextual recall without external service dependencies.

---

## 🤖 Dual-Layer Documentation System

Xavier separates system documentation into two dedicated layers optimized for their respective audiences:

### 1. Codebase Memory (`README.md` & `.md` Files)
This `README.md` and associated markdown assets are structured **strictly as context-injection memory for AI Agents**. They provide exact schema specs, system parameters, architectural constraints, and bootstrapping instructions that agents ingest to operate optimally inside this repository.

### 2. Maloca (Human-in-the-loop Presentation Portal)
For humans, Xavier features **Maloca** (named after the Amazonian indigenous communal house where shared architecture, decisions, stories, and communal knowledge are kept). 
* **Maloca** is the central presentation layer of Xavier's architectural logic, system diagrams, daily release chronicles (ADRs), module breakdowns, and code diff statistics.
* It is compiled into a high-density, static HTML/CSS/JS experience under `public/maloca/` (symlinked to `public/devlog/`).
* It includes a **Premium Interactive Code Diff & Human Curation Dashboard** (`review.html`) where developers can review chronological changes, write curation notes saved in `localStorage`, and inspect exactly which vector RAG memory nodes are linked to each change.

---

## 🛖 The Maloca RAG & Conversation Sync Loop

**Maloca** is not a static log; it is a **circular context database**. 

```
┌────────────────────────┐       ┌────────────────────────┐
│  Git Commit History    │ ────> │  Cosecha de Chronicle  │
└────────────────────────┘       └───────────┬────────────┘
            ▲                                │
            │                                ▼
┌───────────┴────────────┐       ┌────────────────────────┐
│   Despliegue Humano    │ <──── │  Auto-Docs & RAG (BERT)│
│  (public/maloca/)      │       └────────────────────────┘
└────────────────────────┘
```

1. **Automation Hook**: In every git commit, a Husky pre-commit hook runs `scripts/pre-commit-chronicle.sh`.
2. **Context Harvesting**: It harvests commits, code symbols, and git diff statistics, and uses a local BERT embedder to automatically index module understandings into the **Xavier Memory Store**.
3. **Conversational Sync**: The portal integrates development conversation histories. Whenever developers discuss features or decisions with their coding agents, the logs are synchronized directly with Xavier's memory adapters.
4. **Unified Search**: Both human developers and autonomous agents query the same RAG system to instantly retrieve deep historical context about *why* a line of code was changed, what decisions were made, and how components interact.

---

## 🚀 Quick Start (Agent Setup)

Ensure your environment contains the required settings:

```bash
# Set up secure token and workspace path
export XAVIER_TOKEN=your-secure-token
export XAVIER_WORKSPACE_DIR=.

# Launch the memory runtime server (HTTP REST on default port 8006)
xavier serve

# Index the local workspace (creates SQLite database and context-tree.json)
xavier index
```

---

## 🛠️ Installer & Service Support

Xavier includes an interactive **TUI setup wizard** (built with `ratatui` and `crossterm`) that guides you through a 6-step initialization.

### Install Commands

**From source (recommended):**
```bash
cargo install --path . --locked
# or: ./install.sh   # prints the same steps and checks for cargo
```

**Windows (PowerShell as Administrator):**
```powershell
irm https://raw.githubusercontent.com/iberi22/xavier/main/install.ps1 | iex
```

**Linux/macOS helper:** `install.sh` in this repo documents `cargo install` and points at the MCP/HTTP docs. Prefer cloning the repo and installing from source rather than piping remote scripts blindly.

### Start HTTP + optional remote MCP

```bash
xavier http                 # REST :8006; MCP JSON-RPC :8100 by default
xavier http --mcp-port 0    # REST only
xavier mcp                  # MCP stdio for Cursor/Claude (see docs/guides/MCP_INTEGRATION.md)
```

### Background Execution
* **Linux**: Sets up a persistent `systemd` daemon.
* **Windows**: Configures a robust Scheduled Task that launches `xavier serve` / `xavier http` on user logon with automatic retries.

---

## 🔑 Key Architectural Features

- **Belief Graph & GraphRAG** — Dynamic hierarchical clustering (HCE engine) and context-weighted zone boost (1.5x weights for active zones) for intelligent query routing.
- **Embedded BERT & SQLite-vec** — Zero-touch, high-speed local embedding generation (` MiniLM-L6-v2`) writing directly to a vector-enabled SQLite backend.
- **Multi-layered Security Shield** — Proactive scanner analyzing direct, indirect, and semantic threats (prompt injection, path traversal, API key leaks).
- **Interactive Human Curation Dashboard** — Beautiful HTML review dashboard (`review.html`) featuring file diff views, status management, and RAG node links.
- **Model Context Protocol (MCP)** — Stdio (`xavier mcp`) and optional HTTP JSON-RPC (`xavier http --mcp-port`, default `:8100`) for Cursor/Claude.

---

## 📊 Public Dataset Export Schema

Xavier lets you export read-optimized datasets for agent indexing:

```bash
xavier export --public --format tree
```

Outputs lightweight NDJSON streams in `xavier-dataset/` representing:
* `memories.ndjson` — All vector memory records.
* `code_symbols.ndjson` — Structural elements (structs, functions) mapped to files.
* `context-tree.json` — Hierarchical cluster trees representing module architecture.

---

## 📐 System Configuration

Runtime configurations live in `config/xavier.config.json`. Sensitive credentials reside in `.env`.

| Variable | Type | Description |
|---|---|---|
| `XAVIER_TOKEN` | String | Master authentication token for HTTP REST routes |
| `XAVIER_WORKSPACE_DIR` | Path | Root repository path for active indexing operations |
| `XAVIER_EMBEDDING_CACHE_ENABLED` | Boolean | Activates persistent SQLite LRU cache for vector mappings |
| `XAVIER_DEV_MODE` | Boolean | Bypasses HTTP middleware authentication for rapid testing |

---

## 📂 Documentation Manifest

For agents indexing the repository, use these entry paths:
* [Agent Rules (AGENTS.md)](AGENTS.md) — Mandatory guidelines for memory formatting.
* [Xavier Memory Protocol](.agents/skills/xavier-memory-protocol/SKILL.md) — Canonical Fat Search → Page-In → Persist.
* [Feature Status (FEATURE_STATUS.md)](docs/FEATURE_STATUS.md) — Checked-off verified surface.
* [CLI Reference (docs/guides/CLI_REFERENCE.md)](docs/guides/CLI_REFERENCE.md) — Comprehensive command arguments.
* [MCP Integration (docs/guides/MCP_INTEGRATION.md)](docs/guides/MCP_INTEGRATION.md) — Cursor/Claude MCP setup.
* [API Reference (docs/site/.../api.md)](docs/site/src/content/docs/reference/api.md) — HTTP payload specifications.
* [Architecture Guide (docs/ARCHITECTURE.md)](docs/ARCHITECTURE.md) — Hexagonal domain layout.

---

## 🛡️ License

MIT — see [LICENSE](LICENSE) for details. Communal code for autonomous agents.
