# CodeGraph MCP Wiring & Structural Security Audit

**Date**: 2026-07-08
**Author**: Xavier AI
**Tags**: [code-graph, mcp, security, fts5, structural-intelligence, skills]
**Source Files**: [`src/server/mcp/tools_code_graph.rs`](file:///e:/scripts-python/xavier/src/server/mcp/tools_code_graph.rs), [`code-graph/src/db/cypher.rs`](file:///e:/scripts-python/xavier/code-graph/src/db/cypher.rs), [`code-graph/src/main.rs`](file:///e:/scripts-python/xavier/code-graph/src/main.rs), [`.agents/skills/codegraph/SKILL.md`](file:///e:/scripts-python/xavier/.agents/skills/codegraph/SKILL.md)

---

## TL;DR
El commit `7685d810` ("migración a SQLite FTS5 y herramientas MCP") introdujo la infraestructura de CodeGraph y registró 4 herramientas MCP, **pero los handlers eran mocks** que retornaban respuestas falsas prefijadas con `(Mock)` y nunca llamaban al engine real. Este devlog documenta el cableado de los 4 handlers al engine (`code_graph::QueryEngine` / `CodeGraphDB`), la creación del skill de ZCode, un fix en el trazado recursivo, y una auditoría de seguridad y mejoras sobre el codebase de CodeGraph.

---

## Context & Motivation
El proyecto migró su sistema de descubrimiento de código de un archivo JSON estático a una arquitectura de **Inteligencia Estructural** (CodeGraph) respaldada por SQLite + FTS5. Se inyectó en el prompt del agente la regla crítica: *"usa `codegraph_explore` / `trace_path` en vez de búsqueda vectorial/RAG"*.

El problema: la regla instruía a los agentes a usar herramientas que **no funcionaban**. Los 4 handlers en `src/server/mcp/tools_code_graph.rs` retornaban strings hardcodeados:
```rust
let response = format!("(Mock) Explored graph for: {}. ...", query);
```
El engine real (`code_graph::db::CodeGraphDB`, `code_graph::query::QueryEngine`) **sí existía y ya estaba instanciado** en `AppState` como `code_db`, `code_query` e `code_indexer` — solo faltaba llamarlo.

---

## The Decision
1. **Cablear los 4 handlers MCP al engine real** usando el `AppState` ya existente (sin añadir campos).
2. **Crear un skill de ZCode** (`codegraph`) que codifique las reglas duras de uso para todos los agentes.
3. **Fix de `trace_path`** que descartaba rutas alternativas.
4. **Auditar el codebase de CodeGraph** por seguridad y mejoras, dejando los hallazgos documentados.

---

## Deep Dive: Technical Implementation

### 1. Cableado de handlers (`src/server/mcp/tools_code_graph.rs`)
El handler `handle_code_graph_tool(state, ...)` recibe un `AppState` que ya contiene:
```rust
pub struct AppState {
    pub code_indexer: Arc<code_graph::indexer::Indexer>,
    pub code_query: Arc<code_graph::query::QueryEngine>,
    pub code_db: Arc<code_graph::db::CodeGraphDB>,
    ...
}
```
No hizo falta tocar `AppState` ni el dispatcher — solo rellenar los cuerpos de los handlers y renombrar `_state` → `state`.

- **`codegraph_explore`**: llama `state.code_query.search(query, limit)` → `QueryResult`. Enriquece cada símbolo con: slice de fuente leída de disco (numerada por línea, 1-based, con degradación elegante si el archivo falta), callers (`find_edges_to` con `EdgeType::Calls`) y callees (`find_edges_from`). Fallback a `find_by_file` si la query parece una ruta.
- **`trace_path`**: resuelve `symbol` (nombre humano) → `stable_id` vía `code_query.search`, luego llama `code_db.trace_path(stable_id, max_depth, reverse)` y anota cada segmento del `path_str` con etiquetas humanas (`label_for_node`).
- **`get_architecture`**: combina `stats()` + `hub_nodes(3, 15)` + `complexity_hotspots(8.0, 10)` + heurística de entry-points (funciones con 0 aristas `Calls` entrantes).
- **`detect_changes`**: `git diff --name-only HEAD` → `find_by_file` por archivo → `trace_path(reverse=true, depth=2)` para radio de impacto. Degradación elegante si no es repo git.

Los errores de `GraphError` (que implementa `thiserror::Error`) se capturan en un wrapper que devuelve `MCPToolResult::structured(payload, true)` en vez de propagar panic.

### 2. Fix de `trace_path` (`code-graph/src/db/cypher.rs`)
La query original usaba `GROUP BY current_symbol`, lo que descarta rutas alternativas hacia el mismo nodo (SQLite escoge arbitrariamente una fila por grupo). Cambiado a `SELECT DISTINCT` + `ORDER BY depth ASC` para preservar todas las rutas discovered.

### 3. Skill de ZCode (`.agents/skills/codegraph/SKILL.md`)
Skill nuevo con reglas duras: prohíbe RAG/vectorial para preguntas estructurales, documenta las 4 tools y sus args, tabla `reverse` true/false, flujo recomendado (explorar → trazar → confirmar) y anti-patrones. Sigue la convención de `cortex-memory`/`review`.

### 4. Tests de integración (`src/server/mcp/tests.rs`)
Añadidos `code_graph_explore_returns_real_data_not_mock` y `code_graph_trace_path_returns_real_callers`, que indexan un mini-proyecto y verifican que las respuestas **no contienen `(Mock)`** y devuelven datos reales del grafo.

---

## 🔒 Security & Quality Audit

| # | Severidad | Hallazgo | Ubicación | Estado |
|---|---|---|---|---|
| 1 | 🔴 CRÍTICO | Handlers MCP mock retornando respuestas falsas; agentes recibían `(Mock)` | `tools_code_graph.rs:78-109` | ✅ **Arreglado en este PR** |
| 2 | 🟠 ALTA (seguridad) | Token por defecto `"default-token-change-me"` si falta `CODE_GRAPH_TOKEN` | `code-graph/main.rs:310-313` | ⏳ Reportado (follow-up) |
| 3 | 🟠 ALTA (seguridad) | CORS `Any/Any/Any` permite cualquier origen/método/header | `code-graph/main.rs:327-330` | ⏳ Reportado (follow-up) |
| 4 | 🟠 MEDIA | FTS5 `search_code` creado + mantenido por triggers pero **nunca consultado**; `find_symbols` usa `LIKE` + scan | `code-graph/src/db/mod.rs:210-233`, `433-497` | ⏳ Reportado (follow-up) |
| 5 | 🟠 MEDIA | `contains_call` detecta llamadas por substring `name(` → falsos positivos en comentarios/strings/homónimos | `code-graph/src/indexer/mod.rs:289-293` | ⏳ Reportado (follow-up) |
| 6 | 🟡 BAJA | Reindex destructivo (`db.clear()`) antes de indexar; watcher solo loguea (incremental = TODO) | `code-graph/src/indexer/mod.rs:39`, `watcher.rs:44-48` | ⏳ Reportado |
| 7 | 🟡 BAJA | Inconsistencia de rutas DB: MCP usa `data/code_graph.db`; maturity scanner usa `.xavier/codegraph.sqlite` | `src/cli/config.rs:103`, `src/maturity/scanner/code_graph.rs:86` | ⏳ Reportado |
| 8 | 🟡 BAJA | `get_code_graph` legacy (JSON estático `.xavier/codegraph.json`) solapa con las nuevas tools estructurales | `src/server/mcp/tools_core.rs` | ⏳ Reportado (deprecar) |
| 9 | 🟡 BAJA | `QueryEngine::by_language` es stub que retorna `Vec::new()` | `code-graph/src/query/mod.rs:264` | ⏳ Reportado |

### Detalle de los hallazgos HIGH de seguridad (propuesta de fix para follow-up)

**#2 Token por defecto**: el sidecar arranca con `"default-token-change-me"` si no hay env var, imprimiendo solo un warning. Cualquiera en la red local con acceso al puerto 8080 puede invocar `/code/scan`, `/code/find`, `/code/stats`. Propuesta: **reusar el arranque** si no hay token en producción (o generar uno aleatorio efímero y loguearlo una vez).

**#3 CORS abierto**: `CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any)` combinado con el token por defecto débil = superficie de ataque amplia. Propuesta: configurar `CODE_GRAPH_ALLOWED_ORIGINS` (coma-separado) con default restrictivo `http://localhost:*`.

### Nota sobre FTS5 (#4)
El commit se titula "migración a SQLite FTS5", pero la tabla virtual `search_code` (con triggers de sincronización) **no se consulta con `MATCH`** en ningún lugar del crate `code-graph`. `find_symbols` hace `WHERE name LIKE ?1` con `%query%` (full scan de `symbols`) más scoring en Rust. FTS5 es peso muerto: pagas el coste de mantenimiento (triggers en cada insert/update/delete) sin beneficio. O se migra `find_symbols` a `WHERE search_code MATCH ?` (ganando ranking y performance), o se elimina la tabla.

---

## Verification
- `cargo check -p xavier -p code-graph` — ✅ compila limpio (solo warnings preexistentes).
- `cargo test -p code-graph` — ✅ 28 tests pasan.
- `cargo test -p xavier --lib server::mcp` — ✅ 27 tests existentes + 2 nuevos pasan (29 total).
- Los 2 tests nuevos asertan explícitamente que las respuestas **no contienen `(Mock)`**.

---

## Consequences
- Los agentes que reciban la regla "usa `codegraph_explore`/`trace_path`" ahora obtendrán datos reales del grafo en vez de mocks.
- El skill `codegraph` estandariza el comportamiento en todos los hosts ZCode.
- Quedan 8 hallazgos documentados para follow-up; los 2 de seguridad HIGH (token/CORS) son los prioritarios.

---

## Follow-up (2026-07-08, segunda fase) — cierre de hallazgos

Tras el PR #441 se ejecutó la segunda fase de hardening, cerrando los hallazgos pendientes excepto el #6 (sync incremental completo, que queda como primitiva disponible vía `clear_by_file`).

| # | Hallazgo | Resolución |
|---|---|---|
| 2 | Token por defecto débil | ✅ En bind off-loopback sin `CODE_GRAPH_TOKEN` se genera un token efímero aleatorio; el default público solo se permite en loopback con warning. |
| 3 | CORS `Any/Any/Any` | ✅ CORS configurable vía `CODE_GRAPH_ALLOWED_ORIGINS` (coma-separado); default restrictivo a localhost; `*` opts-in explícito al viejo comportamiento. |
| — | Auth con timing leak | ✅ Comparación constant-time (`constant_time_eq`) reemplaza el `==` plano. |
| 4 | FTS5 nunca consultado | ✅ `find_symbols` migrado a `MATCH` + JOIN a `symbols` + `bm25()` con pesos por columna; conserva el ranking exact-match-first de `calculate_score` como capa superior; fallback `LIKE` para DBs legacy y query vacía. |
| 5 | `contains_call` falsos positivos | ✅ Reescrito con word-boundaries: un callee `init` ya no matchea `initialize(` ni `xinit(`. |
| 6 | Reindex destructivo / watcher muerto | 🟡 Añadida primitiva `CodeGraphDB::clear_by_file(path)` (params preparados, no string interp) que habilita sync incremental por-archivo. El watcher sigue siendo TODO pero ahora tiene la primitiva DB necesaria. |
| 7 | Path DB divergente | ✅ El maturity scanner ahora resuelve `code_graph_db_path()` (canónico: `XAVIER_CODE_GRAPH_DB_PATH` → data dir) primero, con fallback legacy `.xavier/codegraph.sqlite`. Ya no lee un DB distinto al del MCP. |
| 8 | `get_code_graph` legacy | ✅ Marcado `[DEPRECATED]` en su descripción MCP, dirigiendo a las tools estructurales nuevas. |
| 9 | `by_language` stub | ✅ Implementado: nuevo `CodeGraphDB::find_by_lang` + `QueryEngine::by_language` delega en él. |

### Detalle técnico del FTS5 (#4)
La tabla `search_code` es external-content (`content='symbols'`, `content_rowid='id'`), así que la query hace JOIN:
```sql
SELECT s.*, bm25(search_code, 10.0, 1.0, 2.0) AS rank
FROM search_code
JOIN symbols s ON s.id = search_code.rowid
WHERE search_code MATCH ?1
ORDER BY rank
LIMIT ?2
```
Pesos `bm25(search_code, name=10, file_path=1, signature=2)` hacen que matches en `name` dominen. La query se envuelve como phrase (`"..."`) para que busquedas multi-palabra matcheen como frase. `calculate_score` se mantiene encima para garantizar exact-match-first (BM25 no da eso gratis).

### Estado final de implementación
Code Graph Index se mantiene en **100% Stable** en la matrix reconciled (`architecture.md`), ahora con los subcomponentes MCP realmente funcionales (no mock). `.gitcore/features.json` actualizado con `last_tested: 2026-07-08` y los nuevos steps. Los test anchors en `.xavier/maturity-anchors.json` apuntan a los tests reales (`code_graph_explore_returns_real_data_not_mock`, `code_graph_trace_path_returns_real_callers`).

### Tests añadidos en esta fase (code-graph: 28 → 32)
- `find_symbols_uses_fts5_matching_signature_column` — verifica que FTS5 matchea la columna `signature`.
- `find_by_lang_returns_only_matching_language`
- `by_language_engine_backed_by_find_by_lang`
- `clear_by_file_removes_only_that_file`

### Cobertura de tests final
- `cargo test -p code-graph` → **32 passed**.
- `cargo test -p xavier --lib server::mcp` → **29 passed** (incluye los 2 de integración que asertan ausencia de mocks).
- `cargo test -p xavier --lib maturity` → **1 passed**.
