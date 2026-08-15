# Anomalías Xavier — Consolidadas para Jules

**Codebase:** `E:\scripts-python\xavier\`  
**Branch:** `main` (`78ba794`) — sincronizado con `origin/main`  
**LOC Rust:** 75,590  
**Fecha:** 2026-06-03

---

## 🔴 Categoría 1: Seguridad (CRÍTICO)

### 1. Issue #428 — Path traversal en conversations_db.rs

**Severidad:** 🔴 CRÍTICO  
**Archivo:** `src/codebase/conversations_db.rs` ~L492-502  
**Problema:** `db_path(project_id: &str)` no sanitiza `project_id`. Un atacante pasa `"../../etc/passwd"` y Xavier escribe/lee `.db` fuera del directorio `~/.xavier/conversations/`.  
**Fix:** Filtrar solo alfanumérico + guiones. Verificar que path resuelto esté dentro del directorio esperado.

---

## 🟡 Categoría 2: Dead Code / Código Muerto

### 2. Issue #426 — Cleanup dead code

**Archivos afectados:**
- `src/secrets/mod.rs` — `SecretsManager` con `#[allow(dead_code)]` y `HashMap<String, String>` sin protección concurrente
- `src/cli/state.rs` — **6** `#[allow(dead_code)]` en líneas 29, 35, 38, 42, 45, 49
- `src/utils/connection_pool.rs` — No se usa (`#[allow(dead_code)]`)
- `src/session/session_store.rs` — Stale
- `src/settings.rs` — `apply_to_env()` muerta

### 3. Issue #425 — Eliminar sistema privacy/public/onboarding

**Archivos:** `src/onboarding/*`, `src/privacy/*`  
**Contexto:** BELA decidió eliminar todo el sistema de `PublishScope`, privacy modes y onboarding. El usuario gestiona público/privado vía GitHub/Git — Xavier no necesita saberlo.

### 4. `#[allow(clippy::...) ]` en `src/onboarding/scanner.rs`

1 `#[allow(unused_imports)]` que debe limpiarse junto con #425.

---

## 🟠 Categoría 3: Refactor Arquitectónico

### 5. Issue #429 — Schema overlap memory/ y codebase/

**Archivos:** `src/memory/sqlite_store.rs`, `src/codebase/*`  
**Problema:** Tablas duplicadas entre módulos `memory/` y `codebase/` — redundancia y riesgo de inconsistencia.

### 6. Issue #424 — ConnectionManager unificado

**Archivos:**
- `src/memory/sqlite_store.rs` — `xavier_memory.db`
- `src/memory/sqlite_vec_store/` — `vec-store`
- `src/codebase/db.rs`
- `src/codebase/conversations_db.rs`

**Problema:** 4+ conexiones SQLite independientes sin gateway común. Riesgo de race conditions, conexiones duplicadas, sin pool.

### 7. PR #430 — Merge phase1-2

**Branch:** `feat/change-control-adr006`  
**+11,795 / -11,574 líneas**, 31 archivos  
**Contiene:** codebase implementation + RFC per-project SQLite + 7 issues para v2 + Turso research + security audit.  
**Estado:** OPEN, necesita revisión.

---

## ⚡ Categoría 4: Performance

### 8. Issue #427 — Patrones Turso

**Archivos:** Todo `src/memory/sqlite*`  
**Propuesta:** Adoptar batch inserts, lazy schema, LRU cache, `spawn_blocking` para operaciones SQLite pesadas. Inspirado en Turso (serverless SQLite).

---

## 📏 Categoría 5: Calidad de Código

### 9. `.expect()` calls — 632 total

| Archivo | Count |
|---------|-------|
| `src/security/prompt_guard.rs` | 153 |
| `src/server/mcp_server.rs` | 68 |
| `src/enterprise/tests.rs` | 47 |
| `src/server/v1_api.rs` | 36 |
| `src/coordination/message_bus.rs` | 32 |
| `src/server/panel.rs` | 17 |
| `src/memory/manager.rs` | 16 |
| `src/enterprise/persistence.rs` | 15 |
| `src/crypto/encryption.rs` | 14 |
| `src/memory/semantic.rs` | 14 |
| `src/memory/belief_graph.rs` | 13 |
| `src/crypto/keys.rs` | 12 |
| `src/memory/episodic.rs` | 12 |
| `src/memory/working.rs` | 11 |
| `src/memory/session_store.rs` | 11 |
| **Otros (15+ archivos)** | ~171 |

### 10. `.unwrap()` calls — 81 total

| Archivo | Count |
|---------|-------|
| `src/memory/pack.rs` | 22 |
| `src/cli/tests.rs` | 14 |
| `src/chronicle/ssg.rs` | 9 |
| `src/agents/extraction.rs` | 6 |
| `src/memory/graph_store.rs` | 5 |
| `src/embedding/cache.rs` | 5 |
| `src/memory/graph_traversal.rs` | 4 |
| `src/agents/provider.rs` | 3 |
| `src/retrieval/gating.rs` | 2 |
| `src/session/auto_save.rs` | 2 |
| `src/agents/system1.rs` | 2 |
| `src/search/rerank.rs` | 1 |
| `src/security/auth.rs` | 1 |
| `src/enterprise/rate_limit.rs` | 1 |
| `src/agents/router.rs` | 1 |
| `src/memory/belief_graph.rs` | 1 |
| `src/enterprise/keys.rs` | 1 |
| `src/enterprise/tests.rs` | 1 |

### 11. Archivos monstruo (>40KB)

| Archivo | Tamaño | Líneas (est.) |
|---------|--------|---------------|
| `src/cli/server.rs` | **95 KB** | ~2,450 |
| `src/server/mcp_server.rs` | **76 KB** | ~2,060 |
| `src/workspace.rs` | **57 KB** | ~1,660 |
| `src/security/prompt_guard.rs` | **44 KB** | ~1,300 |
| `src/server/http.rs` | **44 KB** | ~1,280 |
| `src/retrieval/gating.rs` | **41 KB** | ~1,100 |
| `src/cli/commands.rs` | **39 KB** | ~1,000 |

---

## 📦 Categoría 6: Dependencias

### 12. Dependabot PR #419 — `rusqlite 0.32.1 → 0.40.0`

**Branch:** `dependabot/cargo/rusqlite-0.40.0`  
**+32 / -12 líneas** — Minor bump. Pendiente de merge.

### 13. Dependabot PR #418 — Rust minor updates

**Branch:** `dependabot/cargo/rust-minor-dd041982e2`  
**+103 / -373 líneas** — 4 updates. Pendiente de merge.

---

## 🛠️ Categoría 7: PRs Jules en Progreso

| PR | Estado | Branch | +/- | Título |
|----|--------|--------|-----|--------|
| #433 | OPEN | `chore/cleanup-dead-code-security-enhancement-*` | +11 / -339 | Cleanup dead code + security |
| #432 | OPEN | `refactor-connection-manager-*` | +1,359 / -1,236 | ConnectionManager SQLite |
| #431 | OPEN | `refactor/eliminate-onboarding-privacy-*` | +1 / -1,058 | Eliminar onboarding + YAML |
| #430 | OPEN | `feat/change-control-adr006` | +11,795 / -11,574 | Phase 1-2 merge |

---

## 📊 Resumen

| Categoría | Ítems | Prioridad |
|-----------|-------|-----------|
| 🔴 Seguridad | 1 (path traversal) | **CRÍTICO** |
| 🟡 Dead Code | 3 (issues #426, #425 + scanner) | ALTA |
| 🟠 Refactor | 3 (issues #429, #424 + PR #430) | ALTA |
| ⚡ Performance | 1 (issue #427 — Turso patterns) | MEDIA |
| 📏 Calidad Código | 3 (632 `.expect()` + 81 `.unwrap()` + 7 archivos monstruo) | MEDIA |
| 📦 Dependencias | 2 (PRs #419, #418) | BAJA |
| 🛠️ PRs Progreso | 4 (PRs #433, #432, #431, #430) | EN CURSO |

**Total anomalías identificadas: 17**
