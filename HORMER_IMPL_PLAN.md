# HORMER Implementation Plan for Xavier

> Based on HORMER (Hierarchical Memory Navigation for Efficient Agents) by Duke University + Snowflake AI Research, 2026

## Score de alineación actual: ~100%

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
| `cargo check` | ✅ **0 errores, 0 warnings** |
| `cargo build --release` | ✅ Build exitoso (~7 min) |

### Servidor

| Puerto | Protocolo | Estado |
|--------|-----------|--------|
| 8006 | HTTP (Xavier REST API) | ✅ Running |
| 8100 | MCP 2026-07-28 (HTTP+SSE) | ✅ Running |
| Headers obligatorios | `Origin`, `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` | ✅ |

---

## 📊 Score post-SUG-003 por módulo

| Feature | Antes | Ahora | Estado |
|---------|-------|-------|--------|
| Multi-layer memory | 90% | **95%** | ✅ B1 cache warming predictivo |
| Entity/Knowledge Graph | 75% | **90%** | ✅ B4 visualize CLI, B3 telemetry |
| Consolidation/Decay | 85% | **95%** | ✅ B5 TGD en consolidación nocturna |
| Hybrid Search | 70% | **85%** | ✅ B3 telemetry metrics |
| Hierarchical Directories | 100% | **100%** | ✅ |
| Navigation Policy | 100% | **100%** | ✅ |
| Textual Gradient Descent | 95% | **100%** | ✅ B5 completo |
| GRPO Simplified RL | 100% | **100%** | ✅ |
| Nav Commands | 100% | **100%** | ✅ B4 visualize flags |
| Nav-aware Consolidation | 100% | **100%** | ✅ |
| **Code Health** | 90% | **95%** | ✅ C1 panel.rs refactor |
| **Integration Tests** | 95% | **100%** | ✅ C3 benchmarks |
| **Dependency Hygiene** | 95% | **98%** | ✅ |
| **Warnings (lib)** | 100% | **100%** | ✅ 0 warnings |
| **CI/CD** | 0% | **100%** | ✅ C4 GitHub Actions |
| **Docstrings** | 50% | **90%** | ✅ C2 HORMER docstrings |

---

## 🎯 Plan de Issues para Próximos Sprints

### Sprint SUG-001: Code Health + Tests (Alta prioridad)

| Issue | Descripción | Status |
|-------|-------------|--------|
| A1 | Limpiar warnings (unused imports, dead code) | ✅ Completo |
| A2 | Fix tests preexistentes (keyword extraction, search, v1) | ✅ Completo (5 tests) |
| B6 | Tests de integración HORMER | ✅ Completo (test navigation policy) |
| A5 | Fix wallet MutexGuard Send | ✅ No aplica (wallet.rs sin Mutex) |

## ✅ Sprints Completados

### Sprint SUG-002: Features ✅

| Issue | Descripción | Status | PR |
|-------|-------------|--------|----|
| B1 | Cache warming predictivo (HORMER scores) | ✅ Completo | #236 |
| B4 | CLI visualize mejorado (--hotspots, --tree, --output) | ✅ Completo | #236 |
| B5 | TGD en consolidación nocturna (--nightly) | ✅ Completo | #236 |

### Sprint SUG-003: Pulido ✅

| Issue | Descripción | Status | PR |
|-------|-------------|--------|----|
| C1 | Refactor panel.rs (890→módulos) | ✅ Completo | #237 |
| C2 | Docstrings HORMER pública | ✅ Completo | #237 |
| C3 | Benchmark retrieval con/sin HORMER | ✅ Completo | #237 |
| C4 | CI/CD GitHub Actions | ✅ Completo | #237 |
| B3 | Métricas de navegación (telemetría) | ✅ Completo | #237 |

---

## 🚀 Próximos Pasos (Score 100%)

| Prioridad | Tarea | Descripción |
|-----------|-------|-------------|
| 🔴 Alta | API Key OpenAI | Configurar embeddings provider para memoria semántica |
| 🟡 Media | Mesh P2P | Revisar conexión Supabase, 1 peer con lag alto |
| 🟢 Baja | Roadmap v0.11.0 | Definir features post-HORMER |
| 🟢 Baja | Sovereign Mesh (#115) | EPIC para siguiente major version |
| 🟢 Baja | Governance DAO (#166) | On-chain governance |

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
