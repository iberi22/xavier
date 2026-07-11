# Xavier Architecture

> **GitCore Protocol v3.6.1** | Feature tracking: [.xavier/feature-maturity.json](.xavier/feature-maturity.json)
> Last verified: **2026-07-08** | Overall maturity: **91%** (reconciled tri-source — 7 gaps closed)
> Codebase: 125,734 LOC Rust · 559 archivos · 1044 test fns en 206 archivos · build `cargo test --lib --no-run` ✔

> **Update 2026-07-08 — CodeGraph v0.6.1-beta at 87% overall progress (see code-graph/features.json).** All 7 tree-sitter parsers operational (Rust, TS, Python, Go, Java, C, C++). FTS5+BM25 search with multi-word phrase matching and LIKE fallback. BFS graph traversal with path tracing via recursive CTE. Hub nodes and complexity hotspot analysis. Incremental indexing support (clear_by_file + deterministic stable IDs). Edge detection with heuristic call graph. HTTP sidecar with auth, CORS, path traversal protection. AutoSyncWatcher exists but needs wiring (15%). CI/CD missing (0%).**

> **Reconciliation note (2026-07-02):** the `maturity deep-scan` (scanner v2) reports 6% due to a calibration
> bug — its `test_scanner` reports `tests_passing: 0/0` across every feature despite 1044 verified tests and
> a green build. The manual tracker (`.gitcore/features.json`) reads 75%. A code audit (LOC, module coverage,
> live endpoint verification) reconciles the honest value at **72%**. Fixing the scanner anchors is Phase 0
> of the current sprint (see ROADMAP below).

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
├── storage/       — Storage backends
├── crypto/        — Encryption, hashing
├── security/      — ACL, permissions, secrets
├── sync/          — Memory sync primitives
├── tgd/           — Textual Gradient Descent optimization
└── agents/        — HORMER, evolution, system3

xavier-core/       — Extracted core crate for Android/FFI (PR #207)

code-graph/        — Workspace crate: code-graph v0.6.1-beta
├── src/parser/    — Tree-sitter parsers (Rust, TS, Python, Go, Java, C, C++)
├── src/db/        — SQLite + FTS5 + BM25 + Recursive CTE graphs
├── src/indexer/   — File collection, parsing, edge building
├── src/query/     — QueryEngine + QueryCache (TTL/LRU)
├── src/main.rs    — Sidecar HTTP server (Axum) + CLI commands
└── features.json  — Feature tracking (87% overall progress)
```

## Feature Maturity (reconciled 2026-07-02 — overall 74%)

| Feature | % | Status | Verificado | Sprint Target |
|---------|---|--------|-----------|---------------|
| Hybrid Search (BM25+Vector+RRF) | **100** | 🟢 Stable | automated | — |
| Belief Graph | **100** | 🟢 Stable | automated | — |
| Session Management | **100** | 🟢 Stable | production | — |
| MCP Server (25 tools) | **100** | 🟢 Stable | live 2026-07-02 | — |
| Code Graph Index | **100** | 🟢 Stable | automated | — |
| Encryption at Rest | **100** | 🟢 Stable | automated | — |
| OpenClaw Scanner + Agent CLI | **100** | 🟢 Stable | build | — |
| Docs (Starlight + SRC) | **100** | 🟢 Stable | repo-audit | — |
| Embedding (cloud+local+gllm) | **95** | 🟢 Stable | live 2026-07-02 | — |
| HTTP API REST v1 | **95** | 🟢 Stable | live 2026-07-02 | — |
| Unified SQLite Storage | **85** | 🟡 Beta | production | 95 |
| Clavis Vault + Secrets | **85** | 🟢 Stable | live 2026-07-02 | — |
| Native SDKs (Py/TS) | **80** | 🟡 Beta | repo | 90 |
| Mesh P2P Network | **80** | 🟡 Beta | automated | 95 (Ph 2-4) |
| Notification Persistence | **95** | 🟡 Beta | automated 2026-07-02 | 95 ✅ |
| HORMER Navigation | **90** | 🟡 Beta | automated | 95 |
| Billing/Usage/Provider | **80** | 🟡 Beta | CLI | 90 |
| Auth2 / Token HMAC | **80** | 🟡 Beta | live 2026-07-02 | 90 |
| Data Commons | **75** | 🟡 Beta | automated | 85 |
| Chronicle | **75** | 🟡 Beta | build | 85 |
| TGD Optimization | **70** | 🟡 Beta | scheduler active | 85 |
| Governance DAO | **70** | 🟡 Beta | build | 85 |
| Dual License (MIT+Mesh) | **70** | 🟡 Beta | build | 80 |
| Panel UI (Tauri) | **70** | 🟡 Beta | build | 85 |
| Runtime Health | **85** | 🟡 Beta | live 2026-07-02 | 85 ✅ |
| Telegram Bot | **70** | 🟡 Beta | build 2026-07-02 | 70 ✅ |
| Auto-Improvement Loop | **70** | 🟡 Beta | benchmark 2026-07-02 | 70 ✅ |
| Context Regeneration | **40** | 🟡 Beta | recall@k harness 2026-07-02 | 40 ✅ |
| **Overall (reconciled)** | **74** | | | **86** |

> MCP Server is now 100%: 25 tools verified live on 2026-07-02 (was 48% under the old subcomponent model).
> Sprint closed (2026-07-02): Telegram Bot 35→70%, Notification Persistence 80→95%, Runtime Health 60→85%,
> Auto-Improvement Loop 30→70%, Context Regeneration 0→40% all reached their sprint targets.
> Reconciled overall nudged 72→74% (5 features advanced; scanner v2 still floors at 16% due to a
> known tests_total=0/symbols_found=0 bug — see `.gitcore/features.json`). Largest remaining gap: Mesh Phases 2-4.

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
