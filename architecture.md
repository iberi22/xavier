# Xavier Architecture

> **GitCore Protocol v3.6.1** | Feature tracking: [.xavier/feature-maturity.json](.xavier/feature-maturity.json)
> Sprint JULES-002: 2026-06-18 → 2026-06-25 | Target: 82%
> Overall maturity: **69%**

## Core Modules

```
src/
├── memory/        — RAG engine (SQLite + BM25 + semantic + entity graph)
├── retrieval/     — Search, scoring, gating, navigation policies
├── search/        — BM25, hybrid, reranking, RRF fusion
├── embedding/     — GLLM/OpenAI embedding pipeline
├── codebase/      — Code graph connection manager
├── mesh/          — P2P mesh (HTTP + libp2p transport, governance, ACL)
├── server/        — MCP Server (HTTP+SSE + Stdio)
├── cli/           — CLI commands (server, mcp, code-dump, security)
├── health/        — Self-monitoring & health checks
├── cli/           — Runtime core
├── storage/       — Storage backends
├── crypto/        — Encryption, hashing
├── security/      — ACL, permissions, secrets
├── sync/          — Memory sync primitives
└── tgd/           — Textual Gradient Descent optimization

xavier-core/       — Extracted core crate for Android/FFI (PR #207)
```

## Feature Maturity (v0.11.0 target: 90%)

| Feature | % | Status | Jules | Sprint Target |
|---------|---|--------|-------|---------------|
| Memoria RAG | **85** | ✅ | — | — |
| CLI Tools | **80** | ✅ | — | — |
| Self-monitoring | **80** | ✅ | — | — |
| HORMER Nav | **80** | ✅ | — | — |
| Memory Sync | **75** | ✅ | — | — |
| TGD | **75** | ✅ | — | — |
| Code Graph | **60** | ⚠️ | #210 | 80 |
| Benchmarks | **60** | ⚠️ | #212 | 60 |
| MCP Server | **48** | 🛠️ | #211, #218 | 80 |
| Mesh Network | **45** | 🛠️ | #166, #209, #169 | 60 |
| **Overall** | **69** | | **7 issues** | **82** |

> MCP Server dropped from 80→48% due to 4 new subcomponents (structured output, search/context separation, limits, health tests) added at 0% via feedback from Keesan12 (#195).

## Sprint JULES-002 — Todos los issues asignados a Jules

| # | Issue | Feature | Target % |
|---|-------|---------|----------|
| #166 | Governance DAO on-chain | Mesh | 60% |
| #209 | Data Commons economy | Mesh | 50% |
| #210 | Code Graph dump + MCP | Code Graph | 80% |
| #211 | Docs RAG usage guide | MCP Server | 100% |
| #212 | E2E multi-node tests | Benchmarks | 60% |
| #169 | Dual License design | Mesh | 50% |
| #218 | MCP tools v2: structured output + citations | MCP Server | 80% |

## MCP Server (agent consumption)

### Tools v2 — Best Practices MCP 2026

```
                         ┌─────────────────────┐
                         │     AI Agent         │
                         │  (Claude/GPT/etc)    │
                         └──────┬──────────────┘
                                │ MCP JSON-RPC
                         ┌──────▼──────────────┐
                         │   Xavier MCP Server  │
                         │  (HTTP+SSE / Stdio)  │
                         └──────┬──────────────┘
                     ┌──────────┼─────────────┐
                     ▼          ▼              ▼
              ┌──────────┐ ┌──────────┐ ┌──────────┐
              │mem_search│ │mem_context│ │health    │
              │(candidatos│ │(contexto  │ │check     │
              │+scores)  │ │empaquetado│ │(full)    │
              └──────────┘ └──────────┘ └──────────┘
                     │          │              │
              ┌──────▼──────────▼──────────────▼──┐
              │  Xavier Memory Store (RAG engine)  │
              └───────────────────────────────────┘
```

**Contractos diferenciados:**
- **mem_search** → Candidatos + scores + snippets + provenance. Para que el agente DECIDA.
- **mem_context** → Contenido completo empaquetado + límites. Para INYECTAR en prompt del agente.
- **get_project_context** → Límites explícitos: max_records, max_chars, depth. Truncated flag.

Default: `localhost:7377` | Transports: HTTP+SSE, Stdio
