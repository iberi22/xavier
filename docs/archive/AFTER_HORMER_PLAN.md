# After HORMER — Plan de Seguimiento para Xavier

> Estado: Junio 2026 — HORMER (F1-F6) COMPLETADO
> Score de alineación: ~95%

## ✅ HORMER completado

| Feature | PR | Score |
|---------|----|-------|
| F1 — Directorios jerárquicos | #29 | ✅ 100% |
| F2 — Navigation Policy | #33 | ✅ 100% |
| F3 — Textual Gradient Descent | #32 | ✅ 100% |
| F4 — GRPO simplificado | #33 | ✅ 100% |
| F5 — Comandos shell | #31 | ✅ 100% |
| F6 — Consolidación nav-aware | #27 | ✅ 100% |

## 🐛 Bugs encontrados y corregidos en escaneo

| Bug | Archivo | Fix |
|-----|---------|-----|
| `GatingConfig` sin `navigation_policy` | `api.rs` | Agregado `None` |
| `MutexGuard` no `Send` entre `.await` | `tgd.rs` | Cambiado a `tokio::sync::Mutex` |
| Handler `process_chat` no `Send` | `panel.rs` | Refactor con `tokio::task::spawn` |
| Filenames planos con boost falso | `merger.rs` | F6 fix — no dar boost a `a.md`, `b.md` |
| `settings.save()` síncrono en async | `serialization.rs` | Convertido a `tokio::fs` |

## 📋 Hallazgos del escaneo

### Warnings (3)
1. `simple_index.rs:13` — `Arc` unused import
2. `simple_index.rs:14` — `RwLock` unused import  
3. `graph_traversal.rs:12` — `graph` field never read (Pathfinder)

### Tests preexistentes fallando (5)
- `memory::tests::test_keyword_extraction`
- `memory::tests::test_keyword_scoring`
- `memory::tests::test_search_returns_results`
- `memory::tests::test_token_savings`
- `server::v1_api::tests::test_v1_memories_search_supports_typed_filters_and_user_namespace`

### Errores de compilación en bins (no bloqueantes para lib)
- `xavier-tui` — crate `uuid` no disponible en rlib
- `benchmark` — `STATUS_STACK_BUFFER_OVERRUN`
- `integration` — fallo de compilación
- `quick_embed_test` — `num_traits` no disponible
- `wallet.rs` — `MutexGuard` no Send (código preexistente)

---

## 🎯 Plan de Issues para Fase 2

### Lote A: Code Health (Alta prioridad, ~5 issues)

#### A1: Fix warnings en lib
- **Archivos:** `src/memory/simple_index.rs`, `src/memory/graph_traversal.rs`
- **Qué hacer:** Limpiar unused imports y field dead code
- **Criterio:** `cargo check --lib` → 0 warnings

#### A2: Fix tests preexistentes fallando
- **Archivos:** `src/memory/tests.rs`, `src/server/v1_api.rs`
- **Qué hacer:** Diagnosticar y arreglar los 5 tests que fallan (keyword extraction, token savings, search, v1 filters)
- **Criterio:** `cargo test --lib` → 0 failures

#### A3: Fix compilación TUI bin
- **Archivos:** `Cargo.toml`, features `ratatui`, `crossterm`
- **Qué hacer:** Diagnosticar por qué `uuid` rlib no está disponible en el target test
- **Criterio:** `cargo check --bin xavier-tui` compila

#### A4: Fix compilación benchmarks
- **Archivos:** `tests/benchmark.rs`
- **Qué hacer:** Arreglar stack buffer overrun o crate `futures_timer` faltante
- **Criterio:** `cargo test --test benchmark` compila

#### A5: Fix wallet MutexGuard Send
- **Archivos:** `src/data_commons/wallet.rs`
- **Qué hacer:** Cambiar `std::sync::Mutex` a `tokio::sync::Mutex` para que sea Send-safe
- **Criterio:** `cargo check` compila sin error en wallet.rs

### Lote B: Features faltantes (Media prioridad, ~6 issues)

#### B1: Cache warming predictivo
- **Relevancia HORMER:** El paper habla de pre-calentamiento de memoria basado en patrones
- **Qué hacer:** Implementar predictive cache warming en `src/retrieval/gating.rs`
- **Referencia:** HORMER Section 3.4 — navigation-aware prefetching

#### B2: Adaptive zone boosting por usuario
- **Relevancia:** El sistema de zonas existe pero es estático por workspace
- **Qué hacer:** Hacer que `zone_boost_multiplier` se adapte dinámicamente por usuario/sesión
- **Archivos:** `src/retrieval/gating.rs`, `src/retrieval/config.rs`

#### B3: Métricas de navegación (telemetría)
- **Relevancia:** Sin métricas no podemos validar que HORMER mejora algo
- **Qué hacer:** Agregar counter/histogram de `NavigationPolicy::score` vs resultados reales
- **Archivos:** `src/retrieval/policy.rs`, `src/agents/hormer/reward.rs`

#### B4: CLI avanzado de navegación (`xavier nav visualize`)
- **Relevancia:** El F5 CLI básico está mergeado, falta visualización del grafo
- **Qué hacer:** Agregar subcomando `xavier nav visualize` que renderiza el grafo de memoria
- **Archivos:** `src/cli/commands/navigation.rs`, `src/cli/handlers/navigation.rs`

#### B5: Integración TGD con el pipeline de consolidación
- **Relevancia:** TGD solo se ejecuta cuando confidence < threshold. Debería también correr en consolidación nocturna
- **Qué hacer:** Agregar trigger de TGD en `src/consolidation/mod.rs`
- **Archivos:** `src/consolidation/mod.rs`, `src/agents/tgd.rs`

#### B6: Tests de integración para HORMER
- **Relevancia:** Solo tenemos tests unitarios. Faltan tests de integración end-to-end
- **Qué hacer:** Crear `tests/hormer_integration.rs` con test que: 1) crea docs con directorios, 2) navega, 3) verifica policy update, 4) verifica consolidación nav-aware
- **Archivos:** `tests/hormer_integration.rs` (nuevo)

### Lote C: Deuda técnica (Baja prioridad, ~4 issues)

#### C1: Refactor panel.rs — Separar handlers en módulos
- **Archivos:** `src/server/panel.rs` (~890 líneas)
- **Qué hacer:** Romper en `panel/mod.rs`, `panel/handlers.rs`, `panel/models.rs`

#### C2: Agregar docstrings a todas las funciones públicas de HORMER
- **Archivos:** `src/retrieval/policy.rs`, `src/agents/hormer/*.rs`, `src/agents/tgd.rs`, `src/retrieval/navigation.rs`
- **Qué hacer:** Documentar API pública siguiendo estándar Rust

#### C3: Benchmark de retrieval con y sin HORMER
- **Archivos:** `tests/benchmark.rs`
- **Qué hacer:** Agregar benchmark comparando retrieval speed y quality score con/sin navigation policy

#### C4: CI/CD con GitHub Actions
- **Archivos:** `.github/workflows/ci.yml`
- **Qué hacer:** Pipeline de CI que corre `cargo check --lib`, `cargo test --lib`, clippy, fmt

---

## 📊 Score post-HORMER por módulo

| Feature | Antes | Ahora | Siguiente |
|---------|-------|-------|-----------|
| Multi-layer memory | 85% | 90% | B1 (cache warming) |
| Entity/Knowledge Graph | 70% | 75% | B4 (visualize) |
| Consolidation/Decay | 75% | 85% | B5 (TGD integration) |
| Hybrid Search | 65% | 70% | B2 (adaptive boosting) |
| Hierarchical Directories | 0% | **100%** | ✅ |
| Navigation Policy | 0% | **100%** | ✅ |
| Textual Gradient Descent | 5% | **95%** | ✅ (B5 polishes) |
| GRPO Simplified RL | 0% | **100%** | ✅ |
| Nav Commands | 0% | **100%** | ✅ (B4 polishes) |
| Nav-aware Consolidation | 0% | **100%** | ✅ |
| **Code Health** | — | 60% | Lote A |
| **Integration Tests** | — | 30% | B6 |

## 🚀 Próximo sprint recomendado

### Sprint 1: Code Health + Tests
1. A1 — Limpiar warnings
2. A2 — Fix tests preexistentes
3. B6 — Tests de integración HORMER
4. A5 — Fix wallet Mutex

### Sprint 2: Features
5. B1 — Cache warming predictivo
6. B4 — CLI visualize
7. B5 — TGD en consolidación

### Sprint 3: Pulido
8. C1 — Refactor panel.rs
9. C2 — Docstrings
10. C3 — Benchmarks
11. C4 — CI/CD
12. B3 — Métricas de navegación
