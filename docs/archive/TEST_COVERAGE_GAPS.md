# Test Coverage Gaps — Xavier

> Generated: 2026-06-04  
> Source analysis of `src/` and `tests/`

---

## 1. Resumen de Cobertura Actual

| Métrica | Valor |
|---|---|
| Archivos totales en `src/` | 292 |
| Archivos con tests | 121 (41.4%) |
| Archivos sin tests | 171 (58.6%) |
| Marcadores de test (`#[test]`, `#[cfg(test)]`, `#[tokio::test]`) | 731 |
| Líneas totales en `src/` | ~66,919 |
| Líneas sin cobertura de test | ~23,813 (35.6%) |
| Tests de integración (`tests/`) | 14 archivos ~2,121 líneas |
| Frameworks de test extra detectados | `proptest`, `criterion` |

### Observaciones clave

- **~59% de los archivos no tienen tests unitarios**.
- **~36% del código total** está en archivos sin ningún test.
- Los módulos **memory**, **server**, **cli** y **agents/evolve** son los más críticos.
- Hay **14 tests de integración** cubriendo flujos completos, pero varios módulos grandes carecen de tests unitarios.
- `proptest` y `criterion` ya están en `Cargo.toml` pero subutilizados (solo se usan en un par de módulos).

---

## 2. Módulos Críticos Sin Tests

### Top 11 — Archivos >500 líneas SIN cobertura

| # | Archivo | Líneas | Módulo | Criticidad |
|---|---|---|---|---|
| 1 | `src/cli/server.rs` | 2,703 | CLI / Server | 🔴 **Crítica** — Lógica principal del servidor |
| 2 | `src/server/http.rs` | 1,267 | Server | 🔴 **Crítica** — HTTP server, entradas/salidas |
| 3 | `src/cli/commands.rs` | 1,058 | CLI | 🟠 **Alta** — Comandos, lógica de negocio |
| 4 | `src/memory/qmd/search.rs` | 940 | Memory/QMD | 🔴 **Crítica** — Búsqueda QMD, core del motor de memoria |
| 5 | `src/memory/store.rs` | 814 | Memory | 🔴 **Crítica** — Almacenamiento de memoria general |
| 6 | `src/codebase/conversations_db.rs` | 747 | Codebase | 🟠 **Alta** — Base de datos de conversaciones |
| 7 | `src/agents/system3/helpers/nlp.rs` | 602 | Agents/System3 | 🟠 **Alta** — NLP, procesamiento de lenguaje |
| 8 | `src/enterprise/persistence.rs` | 585 | Enterprise | 🟠 **Alta** — Persistencia empresarial |
| 9 | `src/memory/sqlite_store.rs` | 557 | Memory | 🔴 **Crítica** — Store SQLite de memoria |
| 10 | `src/tasks/session_sync_task.rs` | 552 | Tasks | 🟠 **Alta** — Sincronización de sesiones |
| 11 | `src/memory/qmd/utils.rs` | 520 | Memory/QMD | 🟠 **Alta** — Utilidades del motor QMD |

### Otros archivos relevantes sin tests (entre 200-500 líneas)

| Archivo | Líneas | Módulo |
|---|---|---|
| `src/memory/sqlite_vec_store/store_impl.rs` | 499 | Memory |
| `src/memory/qmd/writer.rs` | 461 | Memory/QMD |
| `src/enterprise/http.rs` | 467 | Enterprise |
| `src/ui/dashboard.rs` | 378 | UI |
| `src/main_tui.rs` | 377 | App |
| `src/agents/system3/helpers/date.rs` | 357 | Agents/System3 |
| `src/billing/mod.rs` | 319 | Billing |
| `src/memory/sqlite_vec_store/backend_impl.rs` | 308 | Memory |
| `src/memory/sqlite_vec_store/schema_impl.rs` | 295 | Memory |
| `src/consolidation/mod.rs` | 275 | Consolidation |
| `src/agents/rate_limit.rs` | 270 | Agents |
| `src/agents/mod.rs` | 252 | Agents |
| `src/adapters/inbound/http/handlers/memory.rs` | 242 | HTTP Handlers |
| `src/app/proxy_use_case.rs` | 235 | App |
| `src/memory/sqlite_vec_store/mod.rs` | 222 | Memory |
| `src/tasks/models.rs` | 213 | Tasks |

---

## 3. Plan de Test para Top 5 Módulos Sin Cobertura

### 🥇 #1: `src/server/http.rs` (1,267 líneas)

**Criticidad:** 🔴 Crítica — Maneja todo el tráfico HTTP entrante.

**Qué testear:**
- Creación y configuración de `Router` con todas las rutas
- Middleware pipeline (auth, rate-limit, logging, CORS)
- Manejo de errores HTTP (400, 401, 403, 404, 500)
- Timeouts y graceful shutdown
- Carga de rutas desde config
- Parsing de query params y request body

**Cómo testear:**
```rust
// Test unitario del router builder
#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;

    #[tokio::test]
    async fn test_router_has_all_routes() { ... }

    #[tokio::test]
    async fn test_health_endpoint_returns_200() { ... }
}
```
- Usar `axum-test` (o `axum_test`) para tests de integración sin levantar servidor real.
- Tests de estrés básico con `#[tokio::test(flavor = "multi_thread")]`.

**Esfuerzo estimado:** 6-8 horas

---

### 🥈 #2: `src/memory/qmd/search.rs` (940 líneas)

**Criticidad:** 🔴 Crítica — Motor de búsqueda principal del sistema de memoria.

**Qué testear:**
- Búsqueda exacta vs fuzzy
- Búsqueda por embeddings (vector similarity)
- Búsqueda híbrida (keyword + vector)
- Filtros por metadatos (tiempo, tipo, fuente)
- Paginación y límites
- Ranking y scoring de resultados
- Casos borde: query vacía, sin resultados, caracteres especiales
- Concurrencia (múltiples búsquedas simultáneas)

**Cómo testear:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_search_finds_expected() { ... }

    #[test]
    fn test_hybrid_search_returns_results() { ... }

    #[tokio::test]
    async fn test_concurrent_searches() { ... }
}
```
- Crear mocks del store subyacente.
- Usar `proptest` para generar queries aleatorias y validar invariantes.

**Esfuerzo estimado:** 8-10 horas

---

### 🥉 #3: `src/cli/server.rs` (2,703 líneas)

**Criticidad:** 🔴 Crítica — Orquestación completa del servidor.

**Qué testear:**
- Inicialización de todos los subsistemas (config, db, memoria, agentes)
- Manejo de señales (SIGTERM, SIGINT)
- Hot-reload de configuración
- Gestión de estado del servidor (starting, running, stopping, crashed)
- Pool de conexiones y resource limits
- Registro de plugins y middlewares

**Cómo testear:**
- Tests unitarios para funciones puras de configuración.
- Tests de integración con un servidor en memoria.
- Separar el estado en structs testeables.
- Usar `tracing-test` para verificar logs en ciertos estados.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_state_transitions() { ... }

    #[tokio::test]
    async fn test_graceful_shutdown_timeout() { ... }
}
```

**Esfuerzo estimado:** 12-16 horas

---

### #4: `src/memory/store.rs` (814 líneas)

**Criticidad:** 🔴 Crítica — Store central de memoria.

**Qué testear:**
- CRUD de entries (create, read, update, delete)
- Bulk operations
- Transacciones / atomicidad
- Límites de tamaño por entry
- TTL y expiración de entries
- Serialización/deserialización
- Consultas por rango de tiempo
- Filtros combinados

**Cómo testear:**
- Tests con store en memoria (usando un trait `MemoryStore`).
- Tests de integración con SQLite y PostgreSQL.
- Usar `proptest` para generar combinaciones de CRUD.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_retrieve_entry() { ... }

    #[tokio::test]
    async fn test_concurrent_read_write() { ... }

    #[test]
    fn test_expired_entries_not_returned() { ... }
}
```

**Esfuerzo estimado:** 6-8 horas

---

### #5: `src/memory/sqlite_store.rs` (557 líneas)

**Criticidad:** 🟠 Alta — Implementación concreta de SQLite.

**Qué testear:**
- Migraciones de schema
- Consultas SQL complejas (joins, subqueries)
- Manejo de errores SQL (constraints, deadlocks, unique violations)
- Pool de conexiones
- Batch inserts y upserts
- FTS (Full Text Search) si aplica

**Cómo testear:**
- Usar SQLite en memoria (`:memory:`) para tests.
- Testear migraciones una por una.
- Pruebas de integración con datos realistas.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_migration_from_v1_to_v2() { ... }

    #[tokio::test]
    async fn test_unique_constraint_violation() { ... }
}
```

**Esfuerzo estimado:** 4-6 horas

---

## 4. Tabla Resumen de Cobertura por Módulo

| Módulo | Archivos | Con Tests | Sin Tests | % Cobertura | Prioridad |
|---|---|---|---|---|---|
| **server/** | 4 | 2 | 2 (http.rs 1267, events.rs 48) | 50% | 🔴 Alta |
| **cli/** | 10 | 2 | 8 (server.rs 2703, commands.rs 1058, ...) | 20% | 🔴 Alta |
| **memory/qmd/** | 10 | 3 | 7 (search.rs 940, utils.rs 520, writer.rs 461, ...) | 30% | 🔴 Alta |
| **memory/** | 19 | 11 | 8 (store.rs 814, sqlite_store.rs 557, ...) | 58% | 🟠 Alta |
| **agents/evolve/** | 7 | 0 | 7 (mod.rs 247, experiment.rs 126, ...) | 0% | 🟠 Alta |
| **agents/system3/helpers/** | 4 | 0 | 4 (nlp.rs 602, date.rs 357, text.rs 158, ...) | 0% | 🟠 Alta |
| **enterprise/** | 7 | 5 | 2 (persistence.rs 585, http.rs 467) | 71% | 🟡 Media |
| **app/** | 6 | 0 | 6 (proxy_use_case.rs 235, security.rs 183, ...) | 0% | 🟡 Media |
| **security/** | 14 | 12 | 2 (detections.rs 143, threat_store.rs 109) | 86% | 🟢 Buena |
| **context/** | 11 | 11 | 0 | 100% | 🟢 Excelente |
| **coordination/** | 4 | 3 | 1 (secrets.rs 140) | 75% | 🟢 Buena |
| **billing/** | 4 | 3 | 1 (mod.rs 319) | 75% | 🟢 Buena |
| **crypto/** | 3 | 2 | 1 (mod.rs 35) | 67% | 🟢 Buena |
| **search/** | 5 | 4 | 1 (mod.rs 5) | 80% | 🟢 Buena |
| **codebase/** | 3 | 2 | 1 (conversations_db.rs 747) | 67% | 🟡 Media |
| **tasks/** | 4 | 1 | 3 (session_sync_task.rs 552, models.rs 213, ...) | 25% | 🟡 Media |

---

## 5. Recomendaciones

### Prioridades inmediatas

1. **`src/server/http.rs`** — Es la puerta de entrada a todo el sistema. Sin tests, cualquier refactor es riesgoso. Agregar `axum-test` o usar `TestServer`.
2. **`src/memory/qmd/search.rs`** — El motor de búsqueda es el feature principal de Xavier. Proptest para cobertura de casos borde.
3. **`src/memory/store.rs`** + **`src/memory/sqlite_store.rs`** — El store es el core de persistencia. Tests unitarios con store in-memory, tests de integración con SQLite real.
4. **`src/cli/server.rs`** — Archivo enorme (2.7k líneas). Separar en módulos más pequeños y testear cada subsistema.

### Framework de test

- Ya tienen **proptest** (v1.11.0) — usarlo más extensivamente para:
  - Generar queries de búsqueda aleatorias
  - Generar estructuras de memoria aleatorias
  - Validar invariantes de serialización
- Ya tienen **criterion** (v0.5) — benchmarks para:
  - `search.rs`: latencia de búsqueda vs número de entries
  - `store.rs`: throughput de escritura/lectura
  - `http.rs`: throughput de requests
- Considerar agregar:
  - **`test-log`**: Para tracing en tests (diagnóstico)
  - **`rstest`**: Test parametrizados (reduce boilerplate)
  - **`mockall`**: Mocks para traits (store, ports)

### Proceso sugerido

- **Semana 1-2:** Tests para `http.rs` + `search.rs` (los más críticos)
- **Semana 3-4:** Tests para `store.rs` + `sqlite_store.rs` (core de persistencia)
- **Semana 5:** Tests para `cli/server.rs` (refactor + tests)
- **Semana 6:** Tests para `agents/evolve/` y `agents/system3/helpers/`
- **Semana 7:** Tests para `app/`, `codebase/conversations_db.rs`, `tasks/`
- **Semana 8:** Benchmark suite para los módulos críticos

### Regla de calidad

- **CRUD paths** en store: 100% coverage de casos base
- **Search paths** en QMD: >90% coverage con proptest
- **HTTP handlers**: happy path + error paths (400, 404, 500)
- **CLI commands**: smoke test de cada comando principal

---

## Apéndice: Tests de Integración Existentes

| Archivo | Líneas | Cubre |
|---|---|---|
| `tests/benchmark.rs` | 140 | Benchmarks varios |
| `tests/chronicle_harvest_test.rs` | 72 | Harvest del chronicle |
| `tests/clavis_integration.rs` | 97 | Integración Clavis |
| `tests/connection_pool_integration.rs` | 81 | Pool de conexiones |
| `tests/integration.rs` | 338 | Integration general |
| `tests/proxy_integration.rs` | 41 | Proxy |
| `tests/rate_limit_integration.rs` | 98 | Rate limiting |
| `tests/server_e2e.rs` | 99 | Server end-to-end |
| `tests/sevier_stress_test.rs` | 380 | Stress test Sevier |
| `tests/storage_isolation.rs` | 33 | Aislamiento de storage |
| `tests/sync_check_handler_cached_result.rs` | 174 | Sync cache |
| `tests/sync_test.rs` | 83 | Sincronización |
| `tests/tui_screenshot_e2e.rs` | 268 | TUI screenshots |
| `tests/websocket_events.rs` | 208 | WebSocket events |

**Total: 14 archivos, ~2,121 líneas de tests de integración.**
