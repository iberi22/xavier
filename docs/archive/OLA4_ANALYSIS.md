# Análisis Ola 4 — Xavier Local-First & Backlog

**Fecha:** 2026-07-17 (actualizado: ola lanzada)  
**Base:** `main@2d6dc39c` (solo `main` en origin/local)  
**Autor:** Orquestador Xavier · skill `jules-async-orchestration`  
**Features:** overall **99.7%** · `feat-local-first` **100%** (post-Ola 3)
**Issues Jules:** #608–#613 triggered · #614 EPIC last (sin jules aún)

---

## 1. Estado de partida (hechos verificados)

### 1.1 Compilación y repo
| Check | Resultado |
| :--- | :--- |
| `cargo check --workspace` | 0 errores (warnings preexistentes) |
| `cargo check --features telegram` | 0 errores (post-#604) |
| Ramas origin | solo `main` |
| PRs open Ola 3 | ninguno |

### 1.2 Ola 3 — balance
| Métrica | Valor |
| :--- | ---: |
| Issues Ola 3 (576–590) | 15 |
| Completados | 14 (incl. #590 panel) |
| Pendientes funcionales | **1 (#578 métricas)** |
| EPIC cierre | #589 blocked |
| Jules fallos de calidad | #603 empty · #601/#602 bloat 56 files |

### 1.3 Porcentajes reconciliados (código vs JSON)

| Feature ID | Antes | Ahora | Evidencia en código |
| :--- | ---: | ---: | :--- |
| `feat-local-first` | 85% | **93%** | 13/14 Ola3 + panel LLM real #605 |
| `feat-runtime-health` | 85% | **93%** | doctor, health, log retention; sin usage metrics |
| `feat-telegram-bot` | 70% | **82%** | `/localstatus` + compile green |
| `feat-mcp-server` | 100% beta | **100%** beta | + `xavier_local_status` |
| `feat-unified-storage` | 95% | **97%** | MigrationV7 + reindex model meta |
| **overall (media)** | 93% | **94%** | mean de 21 features |

Fórmula `feat-local-first`: baseline post-Ola2 **85%** + banda Ola3 (~10pp) × 13/14 ≈ **+9pp → 93%**. Ola4 hot-swap aún 0%.

---

## 2. Deuda que Ola 4 **debe** absorber primero

### P0 — Cierre Ola 3 (no negociable)

#### 2.1 Issue #578 — Métricas de uso reales
**Problema:** no existe `src/observability/usage_counters.rs`. Solo hay `UsageCountersSnapshot` en `src/workspace/usage.rs` (billing/workspace), no instrumentación del proxy LLM.

**Diseño mínimo viable:**
```
src/observability/usage_counters.rs  (NEW)
  - total_requests, total_tokens_in/out, errors
  - per_provider: HashMap<String, ProviderStats>
  - fallback_hops, memory_fallback_hits
  - snapshot() -> serde JSON

src/app/proxy_use_case.rs
  - record success/error/fallback en brazos Ok/Err existentes
  - NO romper circuit breaker (#592)

src/cli/handlers/usage.rs + headless /v1/usage
  - devolver snapshot real

tests unitarios UsageCounters + 1 test de integración ligero
```

**Criterios de aceptación:**
- [ ] Contadores incrementan en `execute_secured`
- [ ] `/v1/usage` (o `xavier usage`) refleja valores reales
- [ ] `cargo check --workspace` 0 errores
- [ ] No tocar `xavier-core/` ni crear `.patch` sueltos

**Merge order:** solo este PR toca `proxy_use_case.rs` en la ola de cierre.

#### 2.2 Issue #589 — EPIC cierre
Después de #578:
- [ ] `feat-local-first` → **96%** (Ola3 completa; Ola4 UI pendiente)
- [ ] ROADMAP Ola 3 → DONE (ya casi actualizado)
- [ ] Devlog técnico en `docs/devlog/`
- [ ] Cerrar #589 y comentar #522

---

## 3. Scope propuesto Ola 4

### Tema: “Control plane local + paridad de fallback + deuda transversal”

| ID tentativo | Título | Prioridad | Archivos clave | Depende de |
| :--- | :--- | :--- | :--- | :--- |
| **4.0** | Métricas de uso (#578) | P0 | `usage_counters.rs`, `proxy_use_case.rs`, usage handlers | — |
| **4.1** | EPIC cierre Ola 3 (#589) | P0 | `features.json`, ROADMAP, devlog | 4.0 |
| **4.2** | Headless memory-fallback parity | P1 | `headless_api.rs` | — |
| **4.3** | Panel: superficie métricas local vs cloud | P1 | `panel-ui/`, usage API | 4.0 |
| **4.4** | Hot-swap Ollama (list/pull/select model) API | P1 | nuevo handler + Ollama HTTP | — |
| **4.5** | Hot-swap UI en panel admin | P1 | `panel-ui/` | 4.4 |
| **4.6** | MCP progressive disclosure polish (#497) | P2 | `tools_memory.rs`, agent rules | — |
| **4.7** | Dependabot triage top-11 high (#478) | P2 | deps / Cargo.lock | — |
| **4.8** | code-graph FTS5 (#445) | P3 | `code-graph/` | — |

### File ownership (anti-conflicto Jules)

| Archivo | Dueño Ola 4 |
| :--- | :--- |
| `src/app/proxy_use_case.rs` | **solo 4.0** |
| `src/cli/handlers/headless_api.rs` | **solo 4.2** |
| `src/observability/*` | 4.0 (+ read-only en 4.3) |
| `panel-ui/**` | 4.3, 4.5 (secuencial) |
| `code-graph/**` | 4.8 |
| `Cargo.toml` / lock | 4.7 |

---

## 4. Riesgos y lecciones Ola 3 (aplicar a Jules)

| Fallo | Mitigación Ola 4 |
| :--- | :--- |
| PR vacío (#603) | Acceptance: “diff debe listar N archivos”; CI `git diff --stat` no vacío |
| Bloat 56 files (#601/#602) | Issue: “DO NOT touch outside this table”; “NO rustfmt whole repo” |
| Draft PRs sin ready | Orquestador: `gh pr ready` antes de merge |
| CI rojo genérico en forks Jules | Gate local obligatorio: `cargo check --workspace` en branch |
| Mismo archivo en 2 PRs | Merge order table + sequential merge |
| Panel stub crítico sin PR | Orquestador implementa P0 si Jules >24h sin PR |

---

## 5. Criterios de salida Ola 4

### Mínimo (MVP Ola 4)
1. #578 merged + tests  
2. #589 closed  
3. `feat-local-first` ≥ **96%** en features.json  
4. `cargo check --workspace` limpio en main  

### Completo (Ola 4 full)
5. Headless memory-fallback parity  
6. Hot-swap API + UI básica  
7. Al menos un ítem P2 (#497 o #478)  

### `feat-local-first` = 100%
Requiere MVP + hot-swap usable (list/select model sin reinicio del proceso).

---

## 6. Orden de ejecución recomendado

```
Semana 0 (inmediato):
  [4.0] #578 UsageCounters  → merge solo
  [4.1] #589 EPIC cierre

Semana 1:
  [4.2] headless memory-fallback
  [4.4] Ollama model API (list/pull/select)
  [4.3] panel métricas (si 4.0 ya en main)

Semana 2:
  [4.5] hot-swap UI
  [4.6] o [4.7] según prioridad de negocio
```

---

## 7. Comandos de verificación (orquestador)

```powershell
git -C e:\proyectosSWAL\xavier checkout main
git -C e:\proyectosSWAL\xavier pull origin main
cargo check --workspace --manifest-path e:\proyectosSWAL\xavier\Cargo.toml
cargo check --manifest-path e:\proyectosSWAL\xavier\Cargo.toml --features telegram

# Features snapshot
(Get-Content e:\proyectosSWAL\xavier\.gitcore\features.json -Raw | ConvertFrom-Json).metadata |
  Select-Object overall_progress_pct, last_verified, reconciliation_notes
```

---

## 8. Decisiones abiertas (para BELA)

1. **¿Ola 4 incluye cost-saving (#497) o se separe como “Ola Cost”?**  
   Recomendación: P2 opcional; no bloquear local-first.
2. **¿Security Dependabot (#478) en paralelo o después de 4.0?**  
   Recomendación: paralelo en worktree distinto (no toca proxy).
3. **¿Jules o implementación orquestador para #578?**  
   Recomendación: orquestador (Jules falló vacío una vez; issue es pequeño y crítico).

---

## 9. Referencias

- Issues: #522, #578, #589  
- PRs Ola 3: #591–#600, #592, #596–#599, #604, #605  
- Memoria Xavier: `ola-3/integration-status-2026-07-17`, `ola-3/cleanup-and-open-backlog-2026-07-17`  
- Skills: `jules-async-orchestration`, `agentic-memory-ops`
