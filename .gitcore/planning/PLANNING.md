# PLANNING.md

**Project:** Xavier — Cognitive Memory Runtime
**Role:** CEO of SWAL (alongside BELA) — Central Memory System for all SWAL agents
**Última actualización:** 2026-07-10

---

## Visión

Xavier es el **sistema de memoria central** de la SWAL ecosystem. No es un plugin ni un sidecar — es el cerebro persistente donde todos los agentes (Codex, Claude, Gemini, Jules, OpenClaw) almacenan y recuperan contexto, decisiones, arquitectura y lecciones aprendidas.

**Filosofía:** "Recall, Analyze, Persist" — cada interacción de agente debe comenzar consultando a Xavier y terminar escribiendo en Xavier.

---

## Tech Stack

| Capa | Tecnología | Propósito |
|------|-----------|-----------|
| Lenguaje | **Rust** (2021 edition) | Performance, seguridad, concurrencia |
| Base de datos | **SQLite** + **sqlite-vec** | Vectores, graph, persistencia, FTS5 |
| Servidor HTTP | **Axum** (0.8) | API REST, Panel UI, health checks |
| Async runtime | **Tokio** (multi-threaded) | I/O concurrente, webhooks, streams |
| Protocolo IA | **MCP** (Model Context Protocol) | 12 tools para LLMs |
| P2P Mesh | **Iroh** / QUIC | Sync distribuida entre nodos Xavier |
| Governance | **DAO Bicameral** | Usuarios (50%) + Consejo (50%) |
| Adjuntos | **PgHeart** | Plugin de monitoreo cardíaco (~/dev/pgheart) |
| Frontend | **React + Vite** (panel-ui/) | Dashboard UI |
| Documentación | **Astro + Starlight** | docs/site/ público |

---

## Cross-References (GitCore Protocol)

| Recurso | Ruta | Propósito |
|---------|------|-----------|
| Source Reference | [.gitcore/SRC.md](../SRC.md) | Estructura de directorios, módulos, entry points |
| Features Tracking | [.gitcore/features.json](../features.json) | 20 features con estado, tests, issues |
| Feature Details | [.gitcore/features/](../features/) | Documentación detallada por feature |
| Config Reference | [.gitcore/SRC_CONFIG.md](../SRC_CONFIG.md) | Variables de entorno y config |
| Rules | [RULES.md](../../RULES.md) | Reglas de codificación, Rust, agentes |
| Agent Rules | [AGENTS.md](../../AGENTS.md) | Identidad, subagentes, protocolo MCP |
| Architecture | [ARCHITECTURE.md](../ARCHITECTURE.md) | Decisiones arquitectónicas no-negociables |
| DevLog | [docs/devlog/](../../docs/devlog/) | Bitácora técnica semanal |
| Documentación | [docs/](../../docs/) | Guías, referencias, whitepapers |
| SDLC Workflow | [SDLC_WORKFLOW.md](../SDLC_WORKFLOW.md) | Ciclo Issue → SRC → Implementación → PR |
| Skills Index | [SKILLS_INDEX.md](../SKILLS_INDEX.md) | Habilidades registradas de Xavier |
| State | [STATE.md](../STATE.md) | Estado global del proyecto |

---

## Fases y Estado Actual

| Fase | Nombre | Objetivo | Estado |
|------|--------|----------|--------|
| F1 | Memoria Core | QMD + Belief Graph + Hybrid Search | ✅ 100% |
| F2 | MCP Integration | HTTP + MCP server para LLMs | ✅ 100% |
| F3 | Multi-tenant | Workspace isolation + quotas | ✅ 100% |
| F4 | Code Indexing | AST-backed symbol search (code-graph) | ✅ 100% |
| F5 | Production Ready | Deployment + Monitoring + Security | 🔄 88% |
| F6 | SQLite Optimization | rtree extension, migration system | ⏳ 20% |
| F7 | Gap Closure | Auto-Improvement, Context Regen, Mesh Ph2-4, Telegram | 🔄 Sprint actual (72→86%) |

> **Overall maturity (reconciled):** 91% (features.json) · 72% (tri-source audit) · Ver ARCHITECTURE.md

---

## Prioridades Q3 2026

### 🥇 Critical (Julio)
1. **Cierre de brechas F7** — Llevar las 7 brechas identificadas a ≥90%
   - Auto-Improvement Loop (85% → 95%)
   - Context Regeneration (60% → 85%)
   - Mesh Network Phase 2-4 (88% → 95%)
   - Telegram Bot (70% → 90%)
2. **Code Graph MCP Wiring** — Completar integración de herramientas MCP con engine real
3. **Dual License hardening** — Tests de persistencia, downgrade, runtime gate

### 🥈 High (Agosto)
4. **SQLite Performance** — rtree extension, columnar indices, migration system v2
5. **Monitoring & Observability** — Prometheus metrics, Grafana dashboard, alertas
6. **Multi-instance sync** — Iroh/QUIC transport con NAT traversal

### 🥉 Medium (Septiembre)
7. **Loro CRDT merge** — Conflict-free merge para memoria compartida
8. **UI Dashboard v2** — Panel UI con notificaciones en tiempo real
9. **Fine-tuning readiness** — Export format, colab integration

---

## Criterios de Éxito (Q3)

1. `cargo test -p xavier` verde (actual: ~943 pass, 10 pre-existing failures)
2. Todos los endpoints documentados en docs/site/
3. Multi-tenant isolation verificado en producción
4. Mesh sync funcional entre 2+ nodos
5. Telegram bot operativo con comandos /memory
6. Auto-Improvement corriendo ciclos autónomos

---

## Riesgos y Mitigación

| Riesgo | Mitigación |
|--------|------------|
| Latencia >500ms en search híbrido | Optimizar RRF, LRU cache, considerar reescritura en C |
| Dependencia de proveedores externos (OpenRouter) | Local GLLM como fallback, AMD GPU soporte |
| Mesh P2P complejidad NAT traversal | Iroh/QUIC con relay servers, STUN/TURN |
| Memory leaks en long-running | Health checks, auto-VACUUM, restart thresholds |
| Breaking changes en API | Endpoints versionados (`/v1/`, `/v2/`) |

---

*Xavier v0.11.0 — Cognitive Memory Runtime · CEO: SWAL*
