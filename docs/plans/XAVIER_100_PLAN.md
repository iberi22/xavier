# 🗺️ Plan para llevar Xavier al 100%

> **Fecha:** 2026-07-12
> **Versión:** 1.0
> **Objetivo:** Cerrar todas las brechas de las 20 features, resolver hallazgos de seguridad, y llevar el maturity reconciliado de 72% → 100%

---

## 📊 Estado Actual

### Features (20 total en features.json)

| Feature | % | Status | Brecha |
|---------|---|--------|--------|
| Hybrid Search | 100% | ✅ Stable | — |
| Belief Graph | 100% | ✅ Stable | — |
| MCP Server | 100% | ✅ Stable | — |
| Code Graph Index | 100% | ✅ Stable | — |
| Session Management | 100% | ✅ Stable | — |
| Encryption at Rest | 100% | ✅ Stable | — |
| Documentation Site | 100% | ✅ Stable | — |
| SRC Reference | 100% | ✅ Stable | — |
| OpenClaw Scanner | 100% | ✅ Stable | — |
| Agent CLI Commands | 100% | ✅ Stable | — |
| **Unified Storage** | **95%** | 🟡 Beta | Migration system, columnar indices |
| **Notification System** | **95%** | 📝 Draft | Edge cases, Tauri real-time |
| **Dual License** | **90%** | 🟡 Beta | Hardening, tests |
| **HORMER Navigation** | **90%** | 🟡 Beta | Alignment score 90→100% |
| **Mesh Network** | **88%** | 🟡 Beta | Phases 2-4 (Iroh/QUIC, Loro, Tor) |
| **Runtime Health** | **85%** | 🟡 Beta | More metrics, alert integrations |
| **Auto-Improvement** | **85%** | 📝 Draft | Full CI integration, more experiments |
| **Telegram Bot** | **70%** | 🟡 Beta | More commands, webhook, production |
| **Governance DAO** | **70%** | 🟡 Beta | On-chain integration, UI, quorum |
| **Context Regeneration** | **60%** | 📋 Planned | Full auto-tune pipeline |

**Total: 10/20 completas · 10/20 por cerrar**

### Security Audit (PR #441)
| # | Severidad | Hallazgo | Estado |
|---|-----------|----------|--------|
| 2 | 🔴 ALTA | Default token en code-graph | ⏳ Pendiente |
| 3 | 🔴 ALTA | CORS any/any/any | ⏳ Pendiente |
| 4 | 🟠 MEDIA | FTS5 search_code creado pero nunca consultado | ⏳ Pendiente |
| 5 | 🟠 MEDIA | contains_call falso positivo por substring | ⏳ Pendiente |
| 6 | 🟡 BAJA | Reindex destructivo, sin incremental | ⏳ Pendiente |
| 7 | 🟡 BAJA | Path inconsistency code_graph.db vs codegraph.sqlite | ⏳ Pendiente |
| 8 | 🟡 BAJA | get_code_graph legacy solapa con tools nuevas | ⏳ Pendiente |
| 9 | 🟡 BAJA | QueryEngine::by_language es stub | ⏳ Pendiente |

### Deuda Técnica
- 10 pre-existing test failures
- `pwsh` faltante en pre-commit hook
- code-graph features al 83% (4 features por cerrar)
- XAVIER_EMBEDDER=cloud obligatorio (pitfall)

---

## 🎯 Filosofía

**Recall, Analyze, Persist.** Cada tarea:
1. Consulta Xavier antes de empezar
2. Implementa con TDD (test → código → commit)
3. Persiste la decisión en Xavier después

---

## Fase I: Low-Hanging Fruit 🍎

> **Duración:** 1-2 sesiones
> **Impacto:** 🟢 Bajo esfuerzo / 🟢 Alto valor

### I.1 Unified Storage — 95% → 100%

**Estado:** SQLite + sqlite-vec production-ready. Missing: migration system, columnar indices.

| Sub-sistema | Actual | Target | Acción |
|---|---|---|---|
| Migration system | Parcial | Completo | Completar MigrationRunner v6 |
| Columnar indices | No | Sí | Añadir índices para queries frecuentes |
| Tests | 0 | 5 | Escribir tests de migración |

**Archivos:**
- `src/storage/migration.rs` — finalizar sistema de migraciones
- `src/storage/schema.rs` — añadir índices columnares
- Tests nuevos en `tests/`

**Criterio de éxito:** `cargo test -p xavier -- feat_unified_storage` verde

---

### I.2 Dual License — 90% → 100%

**Estado:** 5 unit tests, CLI handler, runtime gate. Build verified.

| Sub-sistema | Actual | Target | Acción |
|---|---|---|---|
| Tests | 4 | 5 | Añadir test de runtime gate integración |
| Edge cases | Parcial | Completo | Edge cases: downgrade, persistence |

**Archivos:**
- `src/security/license.rs` — añadir edge cases
- Tests en `src/security/`

**Criterio de éxito:** `cargo test -p xavier -- feat_dual_license` verde

---

### I.3 Notification System — 95% → 100%

**Estado:** Notifier con broadcast::Sender, 4 tests. Missing: edge cases, Tauri real-time.

| Sub-sistema | Actual | Target | Acción |
|---|---|---|---|
| Edge cases | 4 tests | 5+ tests | Error recovery, delivery guarantees |
| Tauri bridge | Stub | Completo | emit_all real-time |

**Archivos:**
- `src/observability/notifier.rs` — edge cases
- `panel-ui/src-tauri/` — Tauri bridge

**Criterio de éxito:** Notificaciones en tiempo real en Panel UI

---

### I.4 Runtime Health — 85% → 100%

**Estado:** Auto-VACUUM, embedding alerts, 7 tests. Missing: more metrics.

| Sub-sistema | Actual | Target | Acción |
|---|---|---|---|
| Disk metrics | Básico | Completo | IOPS, latency, fragmentation |
| Memory pressure | No | Sí | RSS tracking, GC pressure |
| Alert integrations | Embedding only | Multi-channel | Webhook, log, Telegram |

**Archivos:**
- `src/health/mod.rs` — expandir métricas
- `src/observability/` — alert channels

**Criterio de éxito:** Health endpoint cubre CPU, RAM, disk, DB, embedder, mesh

---

## Fase II: Features Core 🏗️

> **Duración:** 2-3 sesiones
> **Impacto:** 🟡 Medio esfuerzo / 🟢 Alto valor
> **Dependencia:** Fase I completa

### II.1 HORMER Navigation — 90% → 100%

**Estado:** 6 features merged, PRs #27-33. ~90% alignment score.

| Sub-sistema | Actual | Target | Acción |
|---|---|---|---|
| Alignment score | ~90% | 100% | TGD final tuning |
| RL policy | Básico | Optimizado | GRPO fine-tuning |
| Navigation-aware consolidation | Parcial | Completo | Cross-cycle persistence |

**Archivos:**
- `src/memory/hormer/` — tuning
- `src/navigation/` — policy optimization
- `src/tgd/` — Textual Gradient Descent

**Criterio de éxito:** Alignment score 100% en benchmarks

---

### II.2 Auto-Improvement — 85% → 100%

**Estado:** AutoImprovementEngine, recall@k benchmark, gap detection. 3 tests.

| Sub-sistema | Actual | Target | Acción |
|---|---|---|---|
| Benchmark runner | 1 harness | Full suite | Más métricas (MRR, precision) |
| CI integration | No | Sí | GitHub Action semanal |
| Experiments | 3 types | 6+ types | Más dimensiones de optimización |
| Tests | 3 | 5+ | Más cobertura |

**Archivos:**
- `src/agents/auto_improvement/` — expandir
- `.github/workflows/auto-improve.yml` — CI integration

**Criterio de éxito:** Ciclo completo benchmark→gap→experiment→validate→merge CI

---

### II.3 Context Regeneration — 60% → 100%

**Estado:** recall@k eval harness + RRF tuner + CLI. Sprint target 40% reached.

| Sub-sistema | Actual | Target | Acción |
|---|---|---|---|
| Benchmark suite | 1 dataset | 3+ datasets | Más queries reales |
| Auto-tune RRF weights | Sí | Completo | Fine-grained por categoría |
| Auto-tune chunk size | No | Sí | Embedding optimization |
| Auto-tune HORMER policy | No | Sí | RL policy auto-tuning |
| Drift detection | Sí | Completo | Alerts + auto-revert |
| Tests | 0 | 5 | Harness tests |

**Archivos:**
- `src/retrieval/eval.rs` — benchmark harness
- `src/retrieval/tuner.rs` — auto-tune
- `src/embedding/` — chunk optimization

**Criterio de éxito:** recall@k ≥ 95% en benchmarks de producción

---

## Fase III: Security & Infrastructure 🛡️

> **Duración:** 2-3 sesiones
> **Impacto:** 🟡 Medio esfuerzo / 🟢 Alto valor
> **Dependencia:** Fase I (Fase II opcional)

### III.1 Security Audit Fixes (PR #441)

| # | Severidad | Hallazgo | Acción |
|---|-----------|----------|--------|
| 2 | 🔴 ALTA | Default token en code-graph | Configurar `CODE_GRAPH_TOKEN` real, validar en runtime |
| 3 | 🔴 ALTA | CORS any/any/any | Restringir a `XavierToken` auth + origen configurable |
| 4 | 🟠 MEDIA | FTS5 search_code no consultado | Cablear `find_symbols` a FTS5 MATCH |
| 5 | 🟠 MEDIA | contains_call falso positivo | AST-based call detection en vez de substring |
| 6 | 🟡 BAJA | Reindex destructivo | Implementar indexación incremental |
| 7 | 🟡 BAJA | Path inconsistency | Unificar paths DB |
| 8 | 🟡 BAJA | get_code_graph legacy | Deprecar, migrar a tools nuevas |
| 9 | 🟡 BAJA | by_language stub | Implementar filtro por lenguaje |

**Archivos clave:**
- `code-graph/src/main.rs` — token, CORS
- `code-graph/src/db/mod.rs` — FTS5 wiring
- `code-graph/src/indexer/mod.rs` — incremental, call detection

---

### III.2 code-graph features — 83% → 100%

Estado actual: 17/21 features complete. Ver `code-graph/features.json` y `code-graph/CODE_GRAPH_100_PLAN.md`.

| Sub-sistema | Actual | Target | Acción |
|---|---|---|---|
| FTS5 search | Stub | Completo | Cablear FTS5 MATCH |
| Incremental indexing | No | Sí | Watcher incremental |
| Language filter | Stub | Completo | Implementar by_language |
| Call detection | Substring | AST-based | Parser-based detection |

---

### III.3 Pre-commit Hook & CI

| Problema | Solución |
|----------|----------|
| pwsh no instalado | Instalar PowerShell Core o cambiar shebang a bash |
| 10 test failures pre-existing | Diagnosticar y reparar |
| XAVIER_EMBEDDER=cloud obligatorio | Documentar como env var requerida en scripts |

---

## Fase IV: Production Features 🚀

> **Duración:** 3-4 sesiones
> **Impacto:** 🔴 Alto esfuerzo / 🟢 Alto valor
> **Dependencia:** Fases I-III

### IV.1 Telegram Bot — 70% → 100%

| Sub-sistema | Actual | Target | Acción |
|---|---|---|---|
| Commands | /memory stats, /memory search | +/health, /status, /regen, /improve | Más comandos |
| Webhook | Sí | Hardened | Auto-reconnect, rate limiting |
| Notifications | No | Sí | Push alerts via notifier |
| Tests | 6 | 10+ | Más cobertura |
| Production readiness | Dev | Deploy | Docker, health checks |

**Archivos:**
- `src/telegram/` — expandir
- `src/observability/notifier.rs` — Telegram channel

---

### IV.2 Governance DAO — 70% → 100%

| Sub-sistema | Actual | Target | Acción |
|---|---|---|---|
| XIP lifecycle | Completo | Hardened | Auto-transiciones, edge cases |
| Weighted voting | Completo | Completo | — |
| On-chain integration | No | Sí | Solana/Ethereum smart contract |
| UI for proposals | No | Sí | Panel UI governance view |
| Quorum detection | No | Sí | Dynamic thresholds |

**Archivos:**
- `src/data_commons/governance.rs` — on-chain bridge
- `src/data_commons/quorum.rs` — nuevo módulo
- `panel-ui/` — governance dashboard

---

### IV.3 Mesh Network — 88% → 100%

| Sub-sistema | Actual | Target | Acción |
|---|---|---|---|
| Phase 0-1 | ✅ Completo | — | — |
| Phase 2: Iroh/QUIC | No | Sí | Transporte P2P real |
| Phase 3: Loro CRDT | No | Sí | Merge conflict-free |
| Phase 4: Tor/Yggdrasil | No | Sí | Transporte anónimo |
| Tests | 0 | 5 | Mesh integration tests |

**Archivos:**
- `src/mesh/transport.rs` — Iroh/QUIC
- `src/mesh/merge.rs` — CRDT
- `src/mesh/relay.rs` — NAT traversal
- `tests/mesh_integration.rs` — expandir

---

## 📊 Mapa de dependencias

```
Fase I (Low-Hanging Fruit 🍎)
├── I.1 Unified Storage  ← No depende de nada
├── I.2 Dual License     ← No depende de nada
├── I.3 Notification     ← No depende de nada
└── I.4 Runtime Health   ← No depende de nada

Fase II (Features Core 🏗️)
├── II.1 HORMER          ← No depende de nada
├── II.2 Auto-Improve    ← I.4 (health metrics)
└── II.3 Context Regen   ← II.2 (benchmark infra)

Fase III (Security 🛡️)
├── III.1 Audit Fixes    ← No depende de nada
├── III.2 code-graph     ← I.1 (DB migrations)
└── III.3 CI/Hooks       ← No depende de nada

Fase IV (Production 🚀)
├── IV.1 Telegram        ← I.4 (notifications)
├── IV.2 Governance      ← I.3 (panel UI)
└── IV.3 Mesh Network    ← I.1 (storage sync)
```

---

## 🚨 Graduation Gates

```
□ Gate 1 — Fase I completa:
  │ □ Unified Storage: cargo test -- feat_unified_storage ✅
  │ □ Dual License: cargo test -- feat_dual_license ✅
  │ □ Notification: Panel UI real-time working
  │ □ Runtime Health: cargo test -- feat_runtime_health ✅
  │
□ Gate 2 — Fase II completa:
  │ □ HORMER: alignment score 100%
  │ □ Auto-Improve: CI cycle running
  │ □ Context Regen: recall@k ≥ 95%
  │
□ Gate 3 — Fase III completa:
  │ □ Security audit: 9/9 resolved
  │ □ code-graph: cargo test ✅, 100% features
  │ □ CI: pre-commit hooks verdes
  │
□ Gate 4 — Fase IV completa:
  │ □ Telegram: cargo test --features telegram ✅
  │ □ Governance: DAO UI operativo
  │ □ Mesh: 2+ nodos sincronizados
```

---

## 📋 Sprint Plan

### Sprint 1: Low-Hanging Fruit (ahora)

| # | Tarea | Archivos | Esfuerzo |
|---|-------|----------|----------|
| 1.1 | Completar migration system Unified Storage | `src/storage/migration.rs` | 🟢 1h |
| 1.2 | Añadir columnar indices | `src/storage/schema.rs` | 🟢 30m |
| 1.3 | Dual License edge cases + tests | `src/security/license.rs` | 🟢 30m |
| 1.4 | Tauri real-time bridge | `panel-ui/src-tauri/` | 🟡 2h |
| 1.5 | Expandir health metrics | `src/health/mod.rs` | 🟡 1h |
| 1.6 | Fix pre-commit hook (pwsh) | `.githooks/pre-push` | 🟢 15m |

### Sprint 2: Features Core

| # | Tarea | Archivos | Esfuerzo |
|---|-------|----------|----------|
| 2.1 | HORMER TGD final tuning | `src/memory/hormer/`, `src/tgd/` | 🟡 2h |
| 2.2 | Auto-Improve CI integration | `.github/workflows/auto-improve.yml` | 🟡 2h |
| 2.3 | Context Regen: benchmark datasets | `src/retrieval/eval.rs` | 🟡 2h |
| 2.4 | Context Regen: auto-tune chunk size | `src/embedding/`, `src/retrieval/tuner.rs` | 🔴 3h |
| 2.5 | Context Regen: drift alerts | `src/retrieval/`, `src/observability/` | 🟡 1h |
| 2.6 | Tests para todas las features core | Varios | 🟡 2h |

### Sprint 3: Security & Infrastructure

| # | Tarea | Archivos | Esfuerzo |
|---|-------|----------|----------|
| 3.1 | Fix default token code-graph | `code-graph/src/main.rs` | 🟢 30m |
| 3.2 | Fix CORS any/any/any | `code-graph/src/main.rs` | 🟢 30m |
| 3.3 | Cablear FTS5 MATCH en find_symbols | `code-graph/src/db/mod.rs` | 🟡 1h |
| 3.4 | AST-based call detection | `code-graph/src/indexer/mod.rs` | 🟡 2h |
| 3.5 | Incremental indexing | `code-graph/src/indexer/watcher.rs` | 🟡 2h |
| 3.6 | Unificar paths DB | `src/cli/config.rs`, `src/maturity/` | 🟢 30m |
| 3.7 | Deprecar get_code_graph legacy | `src/server/mcp/tools_core.rs` | 🟢 15m |
| 3.8 | Implementar by_language | `code-graph/src/query/mod.rs` | 🟢 30m |
| 3.9 | Diagnosticar 10 test failures | Varios | 🟡 1h |

### Sprint 4: Production Features

| # | Tarea | Archivos | Esfuerzo |
|---|-------|----------|----------|
| 4.1 | Telegram: más comandos | `src/telegram/` | 🟡 2h |
| 4.2 | Telegram: push notifications | `src/observability/notifier.rs` | 🟡 1h |
| 4.3 | Governance: on-chain bridge | `src/data_commons/governance.rs` | 🔴 3h |
| 4.4 | Governance: UI dashboard | `panel-ui/` | 🔴 3h |
| 4.5 | Mesh: Iroh/QUIC transport | `src/mesh/transport.rs` | 🔴 4h |
| 4.6 | Mesh: Loro CRDT merge | `src/mesh/merge.rs` | 🔴 3h |
| 4.7 | Mesh: NAT traversal | `src/mesh/relay.rs` | 🔴 2h |
| 4.8 | Mesh integration tests | `tests/mesh_integration.rs` | 🟡 2h |

---

## ⏱️ Timeline sugerido

```
HOY                SEM 1              SEM 2              SEM 3
│                  │                  │                  │
├─ Sprint 1 ───────┤                  │                  │
│  Low-Hanging     │                  │                  │
│  Fruit           ├─ Sprint 2 ───────┤                  │
│                  │  Features Core   │                  │
│                  │                  ├─ Sprint 3 ───────┤
│                  │                  │  Security & Infra │
│                  │                  │                  ├─ Sprint 4 ──►
│                  │                  │                  │  Production
│                  │                  │                  │  Features
```

**Total estimado:** 8-12 sesiones para completar todo

---

## 🧰 Herramientas / Workflow

### TDD obligatorio
Cada tarea de código debe seguir:
1. Escribir test que falla
2. `cargo test` → FAIL
3. Implementar mínimo necesario
4. `cargo test` → PASS
5. `cargo clippy -- -D warnings` → clean
6. Commit atómico

### Commands de verificación
```bash
# Quality gates
cargo.exe fmt --check
cargo.exe check --workspace --all-targets
cargo.exe clippy --workspace --all-targets -- -D warnings
cargo.exe test --workspace --lib

# Feature-specific
cargo test -p xavier -- feat_<feature_name>
cargo test -p xavier --features telegram

# Code-graph
cargo test -p code-graph
cargo test -p xavier -- feat_code_graph_index
```

### Persistencia en Xavier
```bash
# Después de cada tarea completada
curl -s -X POST http://localhost:8006/memory/add \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -d '{"path":"projects/xavier/100-plan/2026-07","content":"Tarea X completada: descripción"}'
```

---

## 📐 Target Architecture (v0.12.0)

```
┌─────────────────────────────────────────────────────────┐
│                    Xavier v0.12.0                         │
│  ┌─────────────┐  ┌──────────┐  ┌───────────────────┐   │
│  │ Memory Core  │  │ MCP      │  │ REST API (Axum)   │   │
│  │ (SQLite-Vec) │  │ Server   │  │ /v1/, /health     │   │
│  │ BM25+FTS5    │  │ 25 tools │  │ WebSocket         │   │
│  │ Belief Graph │  │ SSE+Stdio│  │                   │   │
│  │ HORMER       │  └──────────┘  └───────────────────┘   │
│  └─────────────┘                                          │
│                                                           │
│  ┌─────────────┐  ┌──────────┐  ┌───────────────────┐   │
│  │ Mesh P2P    │  │Telegram  │  │ Governance DAO     │   │
│  │ Iroh/QUIC   │  │ Bot      │  │ On-chain + UI      │   │
│  │ Loro CRDT   │  │ Polling  │  │ XIP lifecycle      │   │
│  │ Tor/Yggdrasil│  │ Webhook  │  │ Quorum detection   │   │
│  └─────────────┘  └──────────┘  └───────────────────┘   │
│                                                           │
│  ┌──────────────────────────────────────────────────┐    │
│  │ Security: Auth2 (JWT+TOTP+BIP39) + AES-GCM+Vault│    │
│  └──────────────────────────────────────────────────┘    │
│                                                           │
│  ┌──────────────────────────────────────────────────┐    │
│  │ Observability: Health + Alerts + Auto-VACUUM    │    │
│  ├──────────────────────────────────────────────────┤    │
│  │ CI/CD: GitHub Actions + Auto-Improve + Benchmarks│    │
│  └──────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘

┌─────────────┐     ┌──────────────┐     ┌──────────────┐
│ code-graph  │────►│ Xavier Core  │◄────│ Panel UI     │
│ (sidecar)   │     │ (orchestrator)│     │ (React+Vite) │
│ AST+FTS5    │     │              │     │ Tauri Desktop│
└─────────────┘     └──────┬───────┘     └──────────────┘
                           │
                    ┌──────▼───────┐
                    │ External     │
                    │ Integrations │
                    │ Telegram, MCP│
                    │ GitHub, CLI  │
                    └──────────────┘
```

---

## 📝 Notas

1. **Priorizar por valor/esfuerzo:** Fase I primero porque da más % con menos esfuerzo
2. **Parallelizable:** Sprints 1 y 3 pueden correr en paralelo (no dependen el uno del otro)
3. **Test failures:** Los 10 pre-existing son PRIORIDAD — sin tests verdes no se puede declarar 100%
4. **v0.12.0 release:** Pushear tag v0.12.0 cuando todas las features estén al 100% para triggerear release.yml
5. **Cada sprint termina con:** `cargo test --workspace` verde + `cargo clippy -- -D warnings` clean

---

*Plan generado: 2026-07-12 · Para ejecutar con subagent-driven-development*
