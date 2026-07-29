# TASK.md

Gestión de Tareas: Xavier
Última actualización: 2026-07-29

## 🔐 Sesión gitcore 2026-07-29 — feat-decentralized-login

| ID | Tarea | Estado | Commit / nota |
|----|-------|--------|----------------|
| — | Baseline tests revalidado | ✅ done | e2e 5/5 · node_fase0_persist 2/2 · node_identity 16/16 · polygon_anchor 8/8 |
| DL-05 | F4 spike fuzzy extractor + ADR go/no-go | ✅ done (70%) | xavier `6747f59e` · veredicto **NO-GO hot-path día 1** (watch-list) |
| DL-06 | UI WebAuthn Maloca (residual UI) | ✅ done | maloca `3ead022` (**pusheado**) · suite `@swal/node` 18/18 |
| DL-03 | Ops Amoy (deploy + smoke broadcast) | ⬜ pending | foundry v1.7.1 instalado (`~/.foundry/bin`); espera key fondeada del usuario (`SWAL_ANCHOR_KEY`) |
| — | Push xavier `main` | ⬜ pending | +4 commits locales (coordina el usuario) |
| — | Sync docs → docs/SWAL | ⬜ pending | tras el push |

**Siguiente acción:** deploy Amoy cuando el usuario provea `SWAL_ANCHOR_KEY` → smoke broadcast → F2 100%.

---

## 🎯 Resumen Ejecutivo y Estado Actual

Estado General: 85% - Proyecto en fase de producción, mantenimiento activo.

Resumen: Xavier v0.4.1 con features core implementadas. Foco en documentación, tests adicionales, y preparación para release 1.0.

## Progreso por Componente

- [x] 🏗️ Motor de Memoria (QMD + Belief Graph): 100%
- [x] 🔍 Hybrid Search (BM25 + Vector): 100%
- [x] 🌐 HTTP API + Panel UI: 100%
- [x] 🔌 MCP Server: 100%
- [x] 👥 Multi-tenant (WorkspaceRegistry): 100%
- [x] 📊 Code Indexing (AST-based): 100%
- [x] 🔐 Token Auth: 100%
- [ ] 📚 Documentación Starlight: 70%
- [ ] 🧪 Cobertura de Tests: 80%
- [ ] 🚀 Deployment Guide: 50%
- [ ] 📊 Monitoring: 30%

---

## 🚀 Fase Actual: Producción + Documentación

Objetivo: llevar Xavier a release 1.0 con documentación completa y tests robustos.

| ID | Tarea | Prioridad | Estado | Issue | Commit |
|----|-------|-----------|--------|-------|--------|
| T-01 | Completar guía de troubleshooting | ALTA | En progreso | - | - |
| T-02 | Expandir API reference con error codes | ALTA | En progreso | - | - |
| T-03 | Crear tutorials de quick-start | MEDIA | Pendiente | - | - |
| T-04 | Docker production deployment guide | MEDIA | Pendiente | - | - |
| T-05 | Agregar tests de stress | MEDIA | Pendiente | - | - |
| T-06 | Monitoring + Prometheus metrics | BAJA | Pendiente | - | - |

---

## ✅ Hitos Completados (v0.4.x)

- **v0.4.1**: Code indexing + bug fixes
- **v0.4.0**: MCP server + multi-tenant
- **v0.3.0**: Hybrid search + belief graph
- **v0.2.0**: Core memory engine

---

## 👾 Deuda Técnica

| ID | Tarea | Prioridad | Estado |
|----|-------|-----------|--------|
| D-01 | Optimizar SQLite con rtree extension para graph queries | MEDIA | Pendiente |
| D-02 | Agregar rate limiting por workspace | MEDIA | Pendiente |
| D-03 | JWT/RBAC activation en security module | BAJA | Pendiente |
| D-04 | Optimizar latencia de search (<200ms target) | MEDIA | Pendiente |

---

## 📋 Issues Activos (`.github/issues/`)

Ver directorio `.github/issues/` para issues individuales.

---

*Xavier v0.4.1*
