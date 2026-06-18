# Xavier Architecture

> **GitCore Protocol v3.6.1** | Feature tracking: [.xavier/feature-maturity.json](.xavier/feature-maturity.json)

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
```

## Feature Maturity (v0.11.0 target)

| Feature | Maturity | Status | Sprint |
|---------|----------|--------|--------|
| Memoria RAG | 85% | ✅ Production Ready | — |
| CLI Tools | 80% | ✅ Production Ready | — |
| Self-monitoring | 80% | ✅ Production Ready | — |
| MCP Server | 80% | ✅ Production Ready | — |
| Memory Sync P2P | 75% | ✅ Production Ready | — |
| HORMER Navigation | 65% | ⚠️ Needs improvement | Sprint Jules |
| Code Graph | 60% | ⚠️ Needs review | Sprint Jules |
| TGD Optimization | 50% | 🛠️ Needs work | Sprint Jules |
| Benchmarks | 50% | 🛠️ In progress | Sprint Jules |
| Mesh Network | 35% | 🛠️ In progress | Sprint Jules |

**Overall: 66%** toward v0.11.0

## Sprint Jules #001

Active: **2026-06-18 → 2026-06-25**

| Issue | Feature | Target |
|-------|---------|--------|
| #198 | TGD nightly consolidation | 80% |
| #199 | HORMER navigation v2 | 80% |
| #196 | Android APK | 60% |
| — | Docs: RAG usage guide | 100% |
| — | Code graph MCP integration | 80% |

## MCP Server (agent consumption)

```
Agent → MCP Client → HTTP+SSE → Xavier MCP Server → Memory/Retrieval/Search
```

Default: `localhost:7377` | Transports: HTTP+SSE, Stdio

See [mcp.rs](src/cli/mcp.rs) for CLI integration.
