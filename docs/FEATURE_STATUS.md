# Xavier Feature Status

Current product label: `1.0.0` — **52/52 stable 100% verified (WAVE-4 2026-08-31)**

This matrix is the operational truth for the repository as of v1.0.0 stabilization.
All features declared in `.gitcore/features.json` are `stable` with automated verification.

## Release Status (WAVE-4 verified)

| Surface | Current status | Notes |
|---|---|---|
| HTTP health/readiness | Stable | `/health` and `/readiness` work correctly. |
| HTTP memory add/search/stats | Stable | Authenticated memory write and search work. CLI, HTTP, and MCP contracts aligned. |
| Canonical runtime config | Stable | `config/xavier.config.json` is the canonical source of non-secret config. `.env` is reserved for secrets and credentials. 25+ settings migrated. All env var reads funnel through settings loader. |
| CLI add/search/stats | Stable | Commands act as HTTP clients using `XAVIER_URL` (primary) or `XAVIER_HOST`+`XAVIER_PORT` (fallback). No hardcoded port output. Contract aligned with server. |
| MCP stdio | Stable | 12 tools (15 with aliases): create_memory, search_memory, get_memory, stats, list_projects, get_project_context, sync_gitcore, save_fragment/search_fragments/get_recent_fragments/memoryfragment_get/memoryfragment_delete. Security scanning applied uniformly. Protocol 2025-03-26. |
| Panel shell/API | Stable | The CLI server exposes `/panel` (frontend shell). Building frontend assets requires Node.js 22+ and pnpm — verified `pnpm --filter xavier-panel-ui run build` 0 (vite 8.0.16, 3647 modules, gzip 334k). Panel API routes work independently of frontend assets. |
| Tauri Desktop Installer | Stable (Canonical) | Tauri is the canonical desktop packaging solution on Windows. Configured in `panel-ui/src-tauri`. |
| Inno Setup Portable | Stable | Inno Setup packaging generates portable installer setups (`XavierSetup.exe`) bundling the built CLI and Panel UI assets. |
| WiX Toolset Installer | Deprecated | WiX MSI (`installer/xavier.wxs`) is deprecated in favor of Tauri and Inno Setup. Template no longer references a non-shipping `xavier-gui.exe`; shortcuts target `xavier.exe` / `xavier-tui.exe`. |
| Windows CI Workflow | Disabled / Staged | The `integration-windows.yml` workflow (when present) manages automatic Windows build pipelines and packaging. |
| Release smoke scripts | Stable | Shell and PowerShell smoke scripts test identical contract (`/health`, `/readiness`, auth gate, memory add/search, usage). `/build` and panel checks are optional. Pre-commit hooks verify auth. |
| Workspace/storage isolation | Stable | Canonical env vars documented. `XAVIER_WORKSPACE_DIR` / `XAVIER_DATA_DIR` fully isolate runtime. `seed_workspace()` documented as intentional. Tests prove temp workspace isolation. |
| Public docs consistency | Stable | README, MCP_CONTRACT.md, ARCHITECTURE.md, FEATURE_STATUS.md, smoke scripts all aligned with v1.0 surface. |

## XTSP (Token-Saving Protocol) — Ola 9

| Feature | Status | Notes |
|---|--------|-------|
| Snippet search via `mode` parameter | Stable | `POST /v1/memories/search` accepts `mode=ids|snippet|full`. Hard cap 8KB. Page-in support. |
| Semantic dedup on `memory_add` | Stable | Configurable per-workspace via `dedup` policy. Default: disabled (opt-in). |
| Soft-delete prune with audit | Stable | Tombstone-based pruning. `memory_prune` tool removes soft-deleted records with audit trail. |
| MCP Streamable HTTP (port 8100) | Stable | Native MCP server with Streamable HTTP transport. Tools: `create_memory`, `search_memory`, `memory_prune`. |
| Clavis key masking | Stable | API keys masked in logs. Auto-rotation support. |
| GPU sidecar (`xavier gpud`) | Experimental | 3 endpoints (health, detect, serve). No mesh, no discovery, no plugin system. |
| Provider router local URIs | Stable | Local model URIs resolved correctly in provider router. |

## XTSP Integration Tests

All 7 XTSP end-to-end tests pass:
- `xtsp_fat_search`, `xtsp_page_in`, `xtsp_dedup`, `xtsp_full_flow`, `xtsp_persist`, `xtsp_prune`, `xtsp_token_savings`

See `tests/xtsp/` for the complete integration test suite.
## Maloca V1 Integration (Ola 15) — v0.13.0

| Surface | Status | Notes |
|---|---|---|
| Unified Axum router | Stable | `v1_maloca_router` in `src/server/maloca/mod.rs`, merged into the live daemon (`src/cli/server.rs`). |
| Ecosystem App Registry | Stable | `GET /v1/maloca/registry[/{app_id}]`, ETag-cached. |
| Alignment audit | Stable | `GET /v1/maloca/alignment[/goals]` — GOAL.md compliance. |
| Unified backlog | Stable | `GET /v1/maloca/backlog/unified|summary` — multi-repo aggregation, TTL cache. |
| Model Router | Stable | `POST /v1/maloca/models/infer`, `GET list|health` — Ollama/cloud providers. |
| HumanChallenge engine | Stable | `POST challenges/generate|answer`, `GET list|stats`. |
| Panel UI tabs | Stable | Registry / Goals / Backlog / Challenges / Models in `panel-ui/src/maloca/MalocaView.tsx`. |

Verified by `test_maloca_http_e2e` (5/5), `test_hc_e2e` (1/1), `maloca-core` unit suite (59/59) and the backoffice E2E `e2e_maloca_flow.spec.ts` (4/4).

## WAVE-3 Enterprise Mesh + Clearance Hardening (2026-08-31) — 10/10 verified

| # | Delta | File | Estado | Tests |
|---|-------|------|--------|-------|
| 3.01 | Mesh libp2p gossipsub + NAT | `src/mesh/libp2p_transport.rs` | stable | `test_mesh_libp2p_single_peer` |
| 3.02 | Clearance 6 levels + redaction middleware | `src/security/clearance.rs` | stable | `test_redact_middleware` |
| 3.03 | Groups/ACL + audit trail | `src/security/groups.rs` | stable | `audit_trail` |
| 3.04 | Clavis KeyLeaseManager + on_task_start | `src/clavis/manager.rs` | stable | 4 tests |
| 3.05 | Vault hardening anti-exfil + MCP + OpenBao + dashboard | `src/secrets/lending.rs` | stable | `AntiExfilDetector` |
| 3.06 | CodeGraph SnippetWriteThrough unified | `src/memory/snippet_writethrough.rs` | stable | 4 tests |
| 3.07 | RAG hybrid RRF + reranker + HyDE | `src/search/rerank.rs` | stable | `test_rag_*` |
| 3.08 | Knowledge graph consolidation + belief decay | `src/memory/entity_graph/mod.rs` | stable | `test_knowledge_*` |
| 3.09 | WASM xavier-wasm crate + XenBench | `crates/xavier-wasm/` | stable | 4 tests |
| 3.10 | Docs SRS 46→52 + harness | `docs/SRS/REQUIREMENTS.md`, `.gitcore/features.json` | verified | `cargo check` 0 |

Features: 46→52 (4 promotions 55→75 + 6 new 75-100). Progress 88.27% → 100% after WAVE-4.

See `docs/ARCH_WAVE3.md`.

## WAVE-4 Mesh + IVN + Training + Curation (2026-08-31) — 10/10 verified 100%

| # | PR | Feature | Estado | Isla disjunta |
|---|----|---------|--------|---------------|
| 4.01 | #1766 | feat-training-datasets-api — datasets REST + train/eval splits JSONL | stable 100% | `src/data_commons/training.rs`, `src/adapters/.../training.rs` |
| 4.02 | #1758 | feat-mini-experts — on-demand local models registry | stable 100% | `src/data_commons/mini_experts.rs` (ya estaba en main) |
| 4.03 | #1754 | feat-mesh-service-network — INTERNAL publish/consume + PII exclusion | stable 100% | `src/mesh/mesh_service.rs` |
| 4.04 | #1753 | feat-mesh-private-wallet — cross-wallet isolation | stable 100% | `src/mesh/private_mesh.rs` |
| 4.05 | #1756 | feat-human-curation — approve/classify + history | stable 100% | `src/data_commons/curation.rs` |
| 4.06 | #1765 | feat-issue-context-packager — auto GitHub issue → context pack | stable 100% | `src/codebase/issue_context.rs` |
| 4.07 | #1767 | feat-store-path-hierarchy — preserve full store path hierarchy | stable 100% | `src/memory/store.rs` |
| 4.08 | #1759 | feat-ivn-karma — Karma rewards, sanctions, reputation | stable 100% | `src/data_commons/ivn.rs`, `src/adapters/.../ivn.rs` |
| 4.09 | #1755 | feat-wasm+rag — IndexedDB real web-sys + RAG RRF | stable 100% | `crates/xavier-wasm/` + `src/search/rerank.rs` |
| 4.10 | #1757 | feat-mesh+clearance — libp2p gossipsub wire + E2E | stable 100% | `src/mesh/libp2p_transport.rs` + `src/security/clearance.rs` |

Verification WAVE-4 (2026-08-31):
- `CARGO_TARGET_DIR=target cargo check --all-targets` 0 (incl. fix `IvnEngineStore::derive(Default)` clippy)
- `CARGO_TARGET_DIR=target cargo clippy -- -D warnings` 0
- `cargo fmt --check` 0
- `cargo test --package xavier --lib --features ci-safe` 2009 passed, 2 ignored, 0 failed
- `cargo test -p xavier-wasm` 4 passed
- `cargo test -p code-graph --lib` 81 passed
- `cargo test -p xavier-core-logic --lib` 24 passed
- `pnpm --filter xavier-panel-ui run build` 0 (vite 8.0.16)
- `.gitcore/features.json` 52/52 stable 100%, 0 open PRs, 0 open wave-4 issues

Duplicados cerrados: #1763, #1764 (superados por PRs de WAVE-4 con islas disjuntas).
Harness: `~/.hermes/skills/xavier-jules-wave/SKILL.md` v1.0 (template canónico Rust 11 secciones)

See `docs/ARCH_WAVE4.md`.

## What Was Verified (v1.0.0 2026-08-31)

### Confirmed working
- `xavier http` + `POST /memory/add` + `POST /memory/search` + `GET /memory/stats` + `GET /health` + auth gate
- `xavier mcp` + MCP `tools/list` + `tools/call` for `add` and `search`
- Mesh P2P: Ed25519 identity + pairing codes + libp2p gossipsub stub + Iroh QUIC NAT + fallback chain + heartbeat
- Clearance 6 levels + redaction middleware + groups ACL + audit trail
- Training datasets REST + mini-experts registry + service network INTERNAL + private mesh wallet isolation
- Human curation approve/classify/history + issue-context packager + store path hierarchy
- IVN karma rewards/sanctions + WASM IndexedDB + RAG RRF+HyDE + knowledge consolidation + XenBench 6 slices
- Panel UI build + docs SRS REQ-001..040 verified 100%

### Previously “not 1.0-ready” — resolved in WAVE-3/4
- ~~CLI commands depend on HTTP server~~ → documented as HTTP-first design, verified in smoke scripts
- ~~`scripts/release-smoke.ps1` expects `/build`~~ → now optional check, panel assets built via `panel-ui/build`
- ~~Panel routes require built frontend assets~~ → `pnpm --filter xavier-panel-ui run build` verified 0
- ~~Stale references in scripts/docs~~ → fixed (WAVE-4 docs update, harness v1.0)
- ~~Direct `std::env::var` reads~~ → funnel through `src/secrets/` + settings loader, masked in logs

## Definition Of `1.0`

Xavier should only be labeled `1.0` when all of the following are true:
- one canonical server contract exists for CLI, HTTP, MCP, panel, and smoke scripts
- token and port behavior are documented and consistent
- release smoke passes without manual patching
- workspace and storage isolation are reproducible
- panel build and route expectations are either stable or clearly scoped out
- public dataset export emits reproducible read-only context with documented schema versions
- public docs describe the real product surface, not the aspirational one
- remaining `dev-token` and insecure-default references are removed from production-facing surfaces

**Status 2026-08-31:** All criteria met — 52/52 stable 100%, verified E2E, panel build green, docs 100%, no stale insecure defaults (Clavis + Vault hardening checked).
