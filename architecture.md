# Xavier Architecture

> **GitCore Protocol v3.6.1** | Feature tracking: [.xavier/feature-maturity.json](.xavier/feature-maturity.json)
> Sprint JULES-002: 2026-06-18 → 2026-06-25 | Target: 85%

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

| Feature | % | Status | Sprint |
|---------|---|--------|--------|
| Memoria RAG | **85** | ✅ Production | — |
| CLI Tools | **80** | ✅ Production | — |
| Self-monitoring | **80** | ✅ Production | — |
| MCP Server | **80** | ✅ Production | Sprint 002 |
| HORMER Nav | **80** | ✅ Production | ✅ Sprint 001 |
| Memory Sync | **75** | ✅ Production | — |
| TGD | **75** | ✅ Production | ✅ Sprint 001 |
| Code Graph | **60** | ⚠️ Needs review | Sprint 002 |
| Benchmarks | **60** | ⚠️ In progress | ✅ Sprint 001 |
| Mesh Network | **45** | 🛠️ In progress | Sprint 002 |
| **Overall** | **78** | | **Target: 85%** |

## Sprint JULES-002

| Issue | Feature | Target | Assigned |
|-------|---------|--------|----------|
| #166 | Governance DAO on-chain | 60% | Jules |
| — | Code graph dump + MCP | 80% | Manual |
| — | Docs RAG via MCP | 100% | Manual |
| — | E2E multi-node | 50% | Manual |
| #169 | Dual License | design | BELA |

## MCP Server (agent consumption)

```
Agent → MCP Client → HTTP+SSE → Xavier MCP Server → Memory/Retrieval/Search
```

Default: `localhost:7377` | Transports: HTTP+SSE, Stdio

See [mcp.rs](src/cli/mcp.rs) for CLI integration.
