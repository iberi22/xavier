# TASK.md

**Project:** Xavier — Cognitive Memory Runtime
**Última actualización:** 2026-07-10
**Fase actual:** F7 — Gap Closure (Sprint: 72% → 86%)
**Estado general:** 91% (features.json) · 72% (tri-source audit)

---

## 🎯 Resumen Ejecutivo

Xavier v0.11.0 con 20 features trackeadas. Core completo (F1-F4), en producción (F5), optimizando brechas (F7). Foco Q3: cerrar auto-improvement, context regeneration, mesh Ph2-4, Telegram.

---

## Progreso por Componente

| Componente | Estado | Progreso | Feature ID |
|-----------|--------|----------|------------|
| 🏗️ Motor de Memoria (QMD) | ✅ Stable | 100% | feat-unified-storage |
| 🔍 Hybrid Search (BM25 + Vector) | ✅ Stable | 100% | feat-hybrid-search |
| 🧠 Belief Graph | ✅ Stable | 100% | feat-belief-graph |
| 🌐 HTTP API + Axum Server | ✅ Stable | 100% | feat-mcp-server |
| 🔌 MCP Server (12 tools) | ✅ Stable | 100% | feat-mcp-server |
| 👥 Multi-tenant | ✅ Stable | 100% | feat-session-management |
| 📊 Code Indexing (AST) | ✅ Stable | 100% | feat-code-graph-index |
| 🔐 Encryption at Rest | ✅ Stable | 100% | feat-encryption-at-rest |
| 📚 Starlight Docs | ✅ Stable | 100% | feat-documentation-site |
| 🧩 Mesh P2P Phase 0-1 | ✅ Stable | 88% | feat-mesh-network |
| 🤖 Telegram Bot | 🟡 Beta | 70% | feat-telegram-bot |
| 🔔 Notification System | 🟡 Draft | 95% | feat-notification-system |
| 🧭 HORMER Navigation | 🟡 Beta | 90% | feat-hormer-navigation |
| 🏛️ Governance DAO | 🟡 Beta | 70% | feat-governance-dao |
| ❤️ Runtime Health | 🟡 Beta | 85% | feat-runtime-health |
| 🔄 Auto-Improvement | 🟡 Draft | 85% | feat-auto-improvement |
| 📜 Dual License | 🟡 Beta | 90% | feat-dual-license |
| 🔁 Context Regeneration | ⏳ Planned | 60% | feat-context-regeneration |
| 🔍 OpenClaw Scanner | ✅ Stable | 100% | feat-openclaw-scanner |
| ⌨️ Agent CLI Commands | ✅ Stable | 100% | feat-agent-cli-commands |

---

## 📋 Tabla de Tareas Activas

| ID | Tarea | Prioridad | Estado | Feature | Issue | GitHub |
|----|-------|-----------|--------|---------|-------|--------|
| T-01 | Cerrar brecha Auto-Improvement 85%→95% | 🔴 ALTA | En progreso | feat-auto-improvement | #322 | - |
| T-02 | Cerrar brecha Context Regen 60%→85% | 🔴 ALTA | En progreso | feat-context-regeneration | - | - |
| T-03 | Mesh Ph2: Iroh/QUIC NAT traversal | 🔴 ALTA | Pendiente | feat-mesh-network | - | - |
| T-04 | Telegram: webhook + /memory commands | 🟡 MEDIA | En progreso | feat-telegram-bot | #84 | - |
| T-05 | SQLite rtree extension + migration system | 🟡 MEDIA | Pendiente | feat-unified-storage | #75 | - |
| T-06 | Prometheus metrics + Grafana | 🟡 MEDIA | Pendiente | feat-runtime-health | - | - |
| T-07 | Governance DAO UI | 🟡 MEDIA | Pendiente | feat-governance-dao | #322 | - |
| T-08 | Code Graph FTS5 polish | 🟡 MEDIA | Pendiente | feat-code-graph-index | #97 | - |
| T-09 | Fine-tuning export format | 🟢 BAJA | Pendiente | - | - | - |
| T-10 | Mesh Ph3: Loro CRDT merge | 🟢 BAJA | Pendiente | feat-mesh-network | - | - |
| T-11 | Mesh Ph4: Tor/Yggdrasil transport | 🟢 BAJA | Pendiente | feat-mesh-network | - | - |
| T-12 | UI Dashboard v2 (notificaciones real-time) | 🟢 BAJA | Pendiente | feat-notification-system | #4 | - |
| T-13 | Plugin System: foundation (Language::Other, traits, types) | 🔴 ALTA | ✅ Completado | feat-plugin-system | - | `bf2dec28` (F1) |
| T-14 | Plugin System: engine + fallback chain | 🔴 ALTA | ✅ Completado | feat-plugin-system | - | `bf2dec28` (F2) |
| T-15 | Plugin System: registry + lifecycle (install/update/rollback) | 🟡 MEDIA | ✅ Completado | feat-plugin-system | - | `ac139304` (F3) |
| T-16 | Plugin System: CLI commands + API endpoints | 🟡 MEDIA | Pendiente → Jules | feat-plugin-system | #487, #488 | F4 |
| T-17 | Plugin System: health monitoring + discovery | 🟡 MEDIA | Pendiente → Jules | feat-plugin-system | #485, #486 | F4 |

---

## ✅ Hitos Completados

| Hito | Versión | Fecha | Features |
|------|---------|-------|----------|
| Core Memory Engine | v0.2.0 | 2026-02 | QMD, Belief Graph, SQLite |
| Hybrid Search | v0.3.0 | 2026-03 | BM25 + Vector + RRF |
| MCP + Multi-tenant | v0.4.0 | 2026-04 | 12 tools, workspace isolation |
| Code Indexing | v0.4.1 | 2026-05 | code-graph sidecar, AST parsing |
| Mesh Phase 0-1 | v0.5.0 | 2026-06 | Identity, ACL, Data Commons |
| Sprint Jul 2026 | v0.11.0 | 2026-07-02 | 7 brechas cerradas, 91% overall |

---

## 👾 Deuda Técnica

| ID | Tarea | Prioridad | Estado | Feature | Impacto |
|----|-------|-----------|--------|---------|---------|
| D-01 | SQLite rtree extension para graph queries | 🟡 MEDIA | Pendiente | feat-unified-storage | Performance |
| D-02 | Rate limiting por workspace | 🟡 MEDIA | Pendiente | feat-session-management | Seguridad |
| D-03 | JWT/RBAC activation | 🟢 BAJA | Pendiente | feat-encryption-at-rest | Seguridad |
| D-04 | Optimizar latencia search <200ms | 🟡 MEDIA | Pendiente | feat-hybrid-search | Performance |
| D-05 | AMD GPU local embedding fallback | 🟢 BAJA | Pendiente | feat-hybrid-search | Resiliencia |
| D-06 | Quorum detection governance | 🟡 MEDIA | Pendiente | feat-governance-dao | Features |
| D-07 | Memory leaks en consolidación | 🟡 MEDIA | Observación | feat-unified-storage | Estabilidad |

---

## 🔗 Vincular Issues de GitHub y Features

| Issue GitHub | Feature | Estado | PR |
|-------------|---------|--------|----|
| #75 | feat-unified-storage | Closed | - |
| #80 | feat-src-reference | Closed | - |
| #81 | feat-session-management | Closed | - |
| #84 | feat-telegram-bot | Open | - |
| #97 | feat-code-graph-index | Closed | #441 |
| #322 | feat-governance-dao | Open | #347 |
| #336 | feat-openclaw-scanner | Closed | #342 |
| #339 | feat-agent-cli-commands | Closed | #342, #345, #346 |
| #4 | feat-notification-system | Open | - |

---

## 📋 Issues Activos Descubiertos (Work-in-Progress)

| ID | Descubrimiento | Feature | Fecha | Estado |
|----|---------------|---------|-------|--------|
| ID-01 | tests_total bug en Scanner v2 (10/0 → 10/10) | feat-openclaw-scanner | 2026-07-02 | ✅ Fixed |
| ID-02 | init_logger duplicado en HTTP server | feat-mcp-server | 2026-06-16 | ✅ Fixed |
| ID-03 | entity_graph blocking Tokio event loop | feat-belief-graph | 2026-06-16 | ✅ Fixed (spawn_blocking) |
| ID-04 | Faltan benchmarks para Context Regen | feat-context-regeneration | 2026-07-03 | 📝 Pendiente |

---

*Xavier v0.11.0 · F7 Gap Closure Sprint*
