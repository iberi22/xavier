# HORMER Implementation Plan for Xavier

> Based on HORMER (Hierarchical Memory Navigation for Efficient Agents) by Duke University + Snowflake AI Research, 2026

## Score de alineación actual: ~95%

| Feature | Score | Status | PR |
|---------|-------|--------|----|
| Multi-layer memory | 90% | ✅ Implementado | — |
| Entity/Knowledge Graph | 75% | ✅ Implementado | — |
| Consolidation/Decay | 85% | ✅ Implementado | — |
| Hybrid Search | 70% | ✅ Implementado | — |
| **F1: Hierarchical Directories** | **100%** | ✅ Mergeado | #29 |
| **F2: Navigation Policy** | **100%** | ✅ Mergeado | #33 |
| **F3: Textual Gradient Descent** | **95%** | ✅ Mergeado | #32 |
| **F4: GRPO Simplified RL** | **100%** | ✅ Mergeado | #33 |
| **F5: Nav Commands (API+CLI)** | **100%** | ✅ Mergeado | #31 |
| **F6: Nav-aware Consolidation** | **100%** | ✅ Mergeado | #27 |

---

## ✅ Sprint JULES-003 — COMPLETO (Junio 2026)

Todos los PRs mergeados, tests pasando, servidor funcional.

### PRs mergeados a `main`

| Commit | PR | Cambio |
|--------|----|--------|
| `2968fee` | #230 | Fix MCP session_tokens schema mismatch |
| `51fba7e` | #229 | Schema validation middleware |
| `074cc8c` | #231 | Fix LTO release crash |
| `896fe2e` | #232 | Eliminar bcrypt, hmac, governor, dashmap |
| `06b3a07` | #233 | Upgrade MCP spec 2026-07-28 |
| `416a8c4` | Fix | Agregar url dep para Origin validation |
| `312df7f` | #224 | Fase 1: once_cell→std, hex/base64 inline |
| `adb8c85` | — | Eliminar pico-args (dead dep) |
| `ae3a90b` | — | Fix 27 MCP tests (headers spec) |
| `d30eb09` | — | Fix MCP test race condition |

### Dependencias eliminadas (9 total)

| Dependencia | Reemplazo |
|-------------|-----------|
| `once_cell` | `std::sync::LazyLock` / `OnceLock` |
| `hex` | `crate::crypto::hex_encode` / `hex_decode` |
| `base64` | `crate::crypto::base64_encode` / `base64_decode` |
| `pico-args` | Código muerto — eliminado |
| `bcrypt` | Inline HMAC-SHA256 + scrypt |
| `hmac` | Inline RFC 2104 |
| `governor` | Token bucket custom |
| `dashmap` | `std::sync::Mutex<HashMap>` |

### Tests

| Suite | Resultado |
|-------|-----------|
| `cargo test --lib --test-threads=1` | ✅ **921 passed, 0 failed, 3 ignored** |
| `cargo check` | ✅ **0 errores, 3 warnings** (chrono::Utc) |
| `cargo build --release` | ✅ Build exitoso (~7 min) |

### Servidor

| Puerto | Protocolo | Estado |
|--------|-----------|--------|
| 8006 | HTTP (Xavier REST API) | ✅ Running |
| 8100 | MCP 2026-07-28 (HTTP+SSE) | ✅ Running |
| Headers obligatorios | `Origin`, `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` | ✅ |

---

## 📊 Score post-HORMER por módulo

| Feature | Antes | Ahora | Siguiente paso |
|---------|-------|-------|---------------|
| Multi-layer memory | 85% | **90%** | B1 (cache warming predictivo) |
| Entity/Knowledge Graph | 70% | **75%** | B4 (visualize CLI) |
| Consolidation/Decay | 75% | **85%** | B5 (TGD en consolidación) |
| Hybrid Search | 65% | **70%** | B2 (adaptive boosting) |
| Hierarchical Directories | 0% | **100%** | ✅ |
| Navigation Policy | 0% | **100%** | ✅ |
| Textual Gradient Descent | 5% | **95%** | ✅ (B5 polishes) |
| GRPO Simplified RL | 0% | **100%** | ✅ |
| Nav Commands | 0% | **100%** | ✅ (B4 polishes) |
| Nav-aware Consolidation | 0% | **100%** | ✅ |
| **Code Health** | — | **90%** | Lote A |
| **Integration Tests** | — | **85%** | B6 |
| **Dependency Hygiene** | — | **95%** | — |

---

## 🎯 Plan de Issues para Próximos Sprints

### Sprint SUG-001: Code Health + Tests (Alta prioridad)

| Issue | Descripción | Status |
|-------|-------------|--------|
| A1 | Limpiar warnings (unused imports, dead code) | 🔵 Pendiente |
| A2 | Fix tests preexistentes (keyword extraction, search, v1) | 🔵 Pendiente |
| B6 | Tests de integración HORMER | 🔵 Pendiente |
| A5 | Fix wallet MutexGuard Send | 🔵 Pendiente |

### Sprint SUG-002: Features

| Issue | Descripción | Status |
|-------|-------------|--------|
| B1 | Cache warming predictivo | 🔵 Pendiente |
| B4 | CLI visualize (`xavier nav visualize`) | 🔵 Pendiente |
| B5 | TGD en consolidación nocturna | 🔵 Pendiente |

### Sprint SUG-003: Pulido

| Issue | Descripción | Status |
|-------|-------------|--------|
| C1 | Refactor panel.rs (890 líneas → módulos) | 🔵 Pendiente |
| C2 | Docstrings HORMER pública | 🔵 Pendiente |
| C3 | Benchmark retrieval con/sin HORMER | 🔵 Pendiente |
| C4 | CI/CD GitHub Actions | 🔵 Pendiente |
| B3 | Métricas de navegación (telemetría) | 🔵 Pendiente |

---

## Issues abiertos actuales

| # | Título | Labels | Estado |
|---|--------|--------|--------|
| 115 | [EPIC] Xavier Sovereign Mesh | jules, mesh-network, enhancement | 📋 Abierta |
| 166 | feat-governance-dao: Bicameral DAO on-chain | jules, mesh-network, design, feat | 📋 Abierta |

---

## Pipeline cíclico

```powershell
# Ejecutar si se crean nuevos issues con label jules
powershell -File C:\Users\belal\.openclaw\skills\jules-integration\scripts\integrate.ps1 -Repo iberi22/xavier -Merge

# Ver estado de PRs:
powershell -File C:\Users\belal\.openclaw\skills\jules-integration\scripts\check-prs.ps1 -Repo iberi22/xavier
```

---

## Estado del Servidor

- **HTTP API**: `http://localhost:8006`
- **MCP**: `http://localhost:8100/mcp`
- **Health**: ✅ Respondiendo
- **Embeddings**: ❌ OpenAI API key no configurada (provider unhealthy)
- **Mesh**: ⚠️ 1 peer conectado (xv1-cloud-supabase, lag alto)
