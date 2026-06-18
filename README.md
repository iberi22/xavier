# Xavier - Fast Vector Memory for AI Agents

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Version](https://img.shields.io/badge/version-0.10.0--12--06--2026-blue.svg)](https://github.com/iberi22/xavier)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![CI](https://img.shields.io/badge/CI-passing-brightgreen.svg)](https://github.com/iberi22/xavier/actions)

Xavier is a **Rust-based context engine and memory runtime for AI agents** with HTTP, CLI, MCP, mesh sync, and data-sharing entry points. It stores, retrieves, curates, and exports vector-backed memory over SQLite/SQLite-Vec, while giving agents a common substrate for context recall, code graph queries, session sharing, and local-first coordination.

Current release: **0.10.0-12-06-2026**.

## Quick Start

```bash
# Install from source
cargo install --path .

# Generate and export an API token
export XAVIER_TOKEN="$(xavier token new | tail -n 1)"

# Start the HTTP server on the default port
xavier http 8006

# In another shell, add and search memory
xavier add "AI agents should verify sources" "agent-guidelines"
xavier search "agent guidelines" --max-results 5

# Check health
curl http://localhost:8006/health
```

Docker:

```bash
docker run --rm -p 8006:8006 \
  -e XAVIER_TOKEN="$XAVIER_TOKEN" \
  -e XAVIER_HOST=0.0.0.0 \
  -v xavier_data:/data \
  ghcr.io/iberi22/xavier:latest
```

MCP stdio:

```bash
xavier mcp
```

## Installer

Xavier ships with an interactive setup wizard for local configuration.

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/iberi22/xavier/main/install.ps1 | iex
```

**Linux/macOS:**

```bash
curl -fsSL https://raw.githubusercontent.com/iberi22/xavier/main/install.sh | bash
```

## Key Features

- **Memory Runtime** - Add, search, update, delete, retrieve, curate, decay, consolidate, reflect, and export agent memory.
- **Mesh Network** - Peer-to-peer sync with node identity, signed handshakes, pairing codes, ACL-aware manifests, chunk transfer, session sharing, cloud settings, and Data Commons opt-in.
- **Data Commons** - Post-quantum encrypted, consent-gated data marketplace and training-bundle workflow for anonymized telemetry and fine-tuning readiness.
- **MCP Server** - stdio-based Model Context Protocol entry point for memory tools and agent integrations.
- **CLI Surface** - Commands for `billing`, `code`, `data-commons`, `mesh`, `navigation`, `provider`, `secrets`, `session`, `spawn`, `tasks`, `token`, `usage`, `verify`, plus core `add`, `search`, `stats`, `http`, `mcp`, and `export`.
- **HTTP API** - Token-protected REST endpoints for memory, mesh, session, code graph, headless automation, panel, secrets, usage, tasks, provider routing, and system health.
- **Code Graph** - Scan codebases, find symbols, inspect dependencies, reverse dependencies, call chains, hubs, hotspots, and stats.
- **Panel UI Backend** - Thread, bookmark, widget, graph, notification, and chat endpoints for the local panel experience.
- **Security & Secrets** - `X-Xavier-Token` auth, security scanning, HMAC token generation, hardware vault commands, and ephemeral secret leases.
- **CI/CD Pipeline** - Multi-OS format/check/clippy/test/build matrix, panel validation and E2E, release smoke tests, multi-architecture Docker images, GitHub release packaging, documentation deployment, and Data Commons E2E checks.

```
┌─────────────┐  ┌──────────┐  ┌──────────┐
│   CLI       │  │  HTTP    │  │   MCP    │
│  (add/search)│ │  Server  │  │  (stdio) │
└──────┬──────┘  └────┬─────┘  └────┬─────┘
       │              │              │
       └──────────────┼──────────────┘
                      │
              ┌───────▼────────┐
              │  Core Engine   │
              │  (add, search, │
              │   stats,       │
              │   export)      │
              └───────┬────────┘
                      │
              ┌───────▼────────┐
              │  SQLite Store  │
              │  + Vector      │
              │  Embeddings    │
              └────────────────┘
```

The CLI, HTTP API, MCP server, mesh transport, and panel backend share the same memory engine. You can run Xavier as a local CLI, an HTTP daemon, a desktop/panel backend, an MCP tool server, or a peer in a mesh network.

## CLI Examples

```bash
xavier stats
xavier export --public --output memories.json
xavier code scan .
xavier code find "MemoryManager" --kind function
xavier mesh id
xavier mesh pairing-code --endpoint http://localhost:8006
xavier data-commons export-training-bundle --output ./training-bundle
xavier verify scan --format markdown --detailed
```

Full CLI reference: [docs/CLI.md](docs/CLI.md).

## HTTP API

All protected endpoints require:

```http
X-Xavier-Token: <your-token>
```

Examples:

```bash
curl http://localhost:8006/health

curl -X POST http://localhost:8006/memory/add \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"content":"Design decision: use RRF","path":"decisions/001"}'

curl -X POST http://localhost:8006/v1/memories/search \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"query":"design decision","limit":5}'
```

Full API reference: [docs/API.md](docs/API.md).

## Mesh Network

```bash
xavier mesh id
xavier mesh pairing-code --endpoint http://node-a:8006
xavier mesh join "<PAIRING_CODE>"
xavier mesh list
xavier mesh sync <node_id> --mode bidirectional
```

Mesh sync uses signed node identities, pairing secrets, peer registries, ACL-aware manifests, and chunk-based transfer. Session bundles can be exported, imported, and shared with trusted peers.

## Data Commons

Data Commons provides consent-gated telemetry export for training and future marketplace workflows:

```bash
xavier data-commons export-training-bundle --output ./bundle --seed 42 --eval-ratio 0.2
xavier data-commons validate ./bundle
```

The v0.10.0 line includes post-quantum encryption design for protected data exchange and token-gated Data Commons automation.

## Public Dataset Export

```bash
xavier export --public --output public-memories.json
```

Context pack export:

```bash
xavier export-pack --topic "mesh sync roadmap" --max-level 3 --out mesh-sync.xcp
```

## Deployment

Xavier can run as a foreground CLI server, Docker container, Docker Compose service, Linux systemd unit, or Windows Scheduled Task. See [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md).

## Configuration

Runtime configuration is read from environment variables and Xavier config files. Secrets should live in environment variables or the vault, not committed files.

| Variable | Default | Description |
|---|---|---|
| `XAVIER_TOKEN` | required for protected HTTP | API token accepted by `X-Xavier-Token` |
| `XAVIER_HOST` | `127.0.0.1` or configured host | HTTP bind host |
| `XAVIER_PORT` | `8006` | HTTP bind port |
| `XAVIER_WORKSPACE_DIR` | platform config dir | Workspace and runtime state directory |
| `XAVIER_MEMORY_BACKEND` | `vec` | Memory backend selector |
| `XAVIER_MEMORY_SQLITE_PATH` | runtime default | SQLite memory path |
| `XAVIER_CODE_GRAPH_DB_PATH` | runtime default | Code graph database path |
| `XAVIER_EMBEDDING_URL` | provider dependent | Embedding API endpoint |
| `XAVIER_MODEL_PROVIDER` | `local` | LLM provider routing default |

## Documentation

- [API Reference](docs/API.md)
- [CLI Reference](docs/CLI.md)
- [RAG Usage Guide](docs/guides/RAG_USAGE_GUIDE.md)
- [Deployment Guide](docs/DEPLOYMENT.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Feature Status](docs/FEATURE_STATUS.md)
- [Public Release Roadmap](docs/PUBLIC_RELEASE_ROADMAP.md)
- [DevLog](docs/devlog/)

## License

Xavier is dual-licensed:
- **MIT License**: For standalone, local-first use of the core memory engine.
- **Xavier Mesh License**: For network participation (Mesh), Governance, Data Commons, and Enterprise features. Free for individuals/OSS; paid for commercial entities above certain thresholds.

See [LICENSE](LICENSE) and [LICENSE-MESH](LICENSE-MESH) for details. Commercial terms are documented in [docs/PRICING.md](docs/PRICING.md).
