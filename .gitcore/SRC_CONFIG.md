# SRC Configuration - Xavier

**Version:** 1.2
**Date:** 2026-07-21
**Project:** Xavier Cognitive Memory System (v0.12.0)

---

## 1. Purpose

This document defines the configuration reference for Xavier, including runtime settings, environment variables, and module-specific requirements. It serves as the authoritative reference for deploying, tuning, and troubleshooting Xavier instances.

---

## 2. Configuration Files

### Core Runtime Configuration

**Location:** `config/xavier.config.json` (default, overridable via `XAVIER_CONFIG_PATH`)

**Schema:** Managed via `XavierSettings` struct in `src/settings/mod.rs`

#### Top-Level Sections

| Section | Type | Description |
|---------|------|-------------|
| `server` | object | HTTP server settings (host, port, log level, CORS) |
| `workspace` | object | Multi-tenant workspace defaults |
| `memory` | object | Memory backend configuration (vec store, SQLite path) |
| `memory_layers` | object | Working, Episodic, Semantic layer parameters |
| `models` | object | LLM and Embedding provider settings |
| `retrieval` | object | Learned policy weights, search multipliers |
| `security` | object | Auth, TOTP, rate limiting, prompt guard |
| `telegram` | object | Telegram bot token and chat mappings |
| `sync` | object | Intervals and thresholds for data sync |
| `data_commons` | object | Governance and data sharing settings |
| `mesh` | object | P2P networking configuration |

#### Key Fields Reference

```json
{
  "server": {
    "host": "0.0.0.0",
    "port": 8006,
    "log_level": "info",
    "cors_origins": ["*"],
    "max_body_size": 10485760
  },
  "memory": {
    "backend": "vec",
    "sqlite_path": "data/xavier.db",
    "embedding_dimensions": 768,
    "fts_enabled": true
  },
  "memory_layers": {
    "working": { "capacity": 100, "ttl_seconds": 3600 },
    "episodic": { "capacity": 10000, "ttl_seconds": 86400 },
    "semantic": { "unlimited": true }
  },
  "models": {
    "chat": { "provider": "ollama", "model": "llama3.1" },
    "embedding": { "provider": "ollama", "model": "nomic-embed-text" }
  },
  "retrieval": {
    "learned_policy": {
      "working_weight": 0.33,
      "episodic_weight": 0.33,
      "semantic_weight": 0.34,
      "learning_rate": 0.01
    },
    "search": {
      "hybrid_alpha": 0.7,
      "rrf_k": 60,
      "default_limit": 10
    }
  }
}
```

### Environment Variables

| Variable | Default | Required | Description |
|---|---|---|---|
| `XAVIER_TOKEN` | — | Yes (prod) | Authentication token for HTTP API (bearer token) |
| `XAVIER_CONFIG_PATH` | `config/xavier.config.json` | No | Override path for configuration JSON |
| `XAVIER_PORT` | `8006` | No | Default HTTP server port |
| `XAVIER_CODE_GRAPH_DB_PATH` | `data/code-graph.db` | No | Code-graph sidecar database path |
| `XAVIER_LOG_DIR` | `~/.xavier/logs` | No | Log output directory (auto-rotated) |
| `XAVIER_DATA_DIR` | `~/.xavier/data` | No | Runtime data directory (DBs, vector stores) |
| `XAVIER_OPENAI_KEY` | — | No | OpenAI API key (if using OpenAI provider) |
| `XAVIER_OLLAMA_URL` | `http://localhost:11434` | No | Ollama server URL |
| `XAVIER_GEMINI_KEY` | — | No | Google Gemini API key |
| `XAVIER_MESH_KEY_PATH` | `~/.config/xavier/mesh.key` | No | Ed25519 identity key for mesh networking |

---

## 3. Module Requirements

### `src/agents/` — Agent Runtime
- **Requirements:** Active LLM provider (Ollama, OpenAI, Gemini, or OpenRouter), memory storage access
- **Optional:** Rate limit manager for multi-tenant deployments

### `src/memory/` — Hierarchical Memory
- **Requirements:** SQLite with `sqlite-vec` extension (bundled via `rusqlite`), valid embedding model
- **Runtime:** `XAVIER_DATA_DIR` writable for database files

### `src/embedding/` — Vector Embeddings
- **Requirements:** At least one embedding provider configured (Ollama, OpenAI, or GLLM)
- **Fallback:** `NoopEmbedder` for testing (returns zero vectors)

### `src/security/` — Security Layer
- **Requirements:** `XAVIER_TOKEN` set for production, TOTP secret for 2FA (optional)
- **License:** MIT (default, no restrictions) or Mesh License (requires acceptance)

### `src/mesh/` — P2P Networking
- **Requirements:** Ed25519 identity key (auto-generated), network connectivity for peer discovery

### `src/retrieval/` — Navigation & Search
- **Requirements:** Memory backend with FTS5 and vector indexing enabled
- **Tuning:** `retrieval.learned_policy` weights auto-tuned by HORMER GRPO

---

## 4. Requirement IDs (SRC Mapping)

| ID | Module | Requirement | Status |
|----|--------|-------------|--------|
| XAV-CORE-001 | server | Must support authenticated HTTP/REST API | implemented |
| XAV-CORE-002 | server | Must support MCP stdio protocol | implemented |
| XAV-MEM-001 | memory | Must support hybrid BM25 + Vector search | implemented |
| XAV-MEM-002 | memory | Must persist across restarts (SQLite) | implemented |
| XAV-MEM-003 | memory | Must support 3 memory layers (working/episodic/semantic) | implemented |
| XAV-COG-001 | agents | Must implement System 3 reasoning oversight | implemented |
| XAV-COG-002 | agents | Must support multi-provider LLM routing | implemented |
| XAV-COG-003 | agents | Must support learned navigation policy (HORMER) | implemented |
| XAV-MSH-001 | mesh | Must synchronize memories across peers | beta |
| XAV-MSH-002 | mesh | Must support Ed25519 identity verification | implemented |
| XAV-SEC-001 | security | Must detect and block prompt injection | implemented |
| XAV-SEC-002 | security | Must support session-based auth with token rotation | implemented |
| XAV-SEC-003 | security | Must enforce dual license (MIT / Mesh) | implemented |
| XAV-ENT-001 | enterprise | Must support RBAC for multi-tenant deployments | implemented |
| XAV-ENT-002 | enterprise | Must provide audit logging | implemented |
| XAV-OBS-001 | observability | Must expose Prometheus metrics | implemented |
| XAV-OBS-002 | observability | Must support structured logging (tracing) | implemented |

---

## 5. Build & Deployment Metadata

```yaml
src_version: "1.2"
project: "Xavier"
version: "0.12.0"
config_schema: "v2"
rust_edition: "2024"
min_rust: "1.85"
features:
  - hybrid-search
  - hormer-navigation
  - mesh-p2p
  - governance-dao
  - prompt-guard
  - dual-license
  - mcp-server
  - notification-system
  - context-regeneration
  - e2e-tests
```

---

*Generated by Hermes Agent — Last updated: 2026-07-21*