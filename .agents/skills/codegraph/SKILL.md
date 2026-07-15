---
name: codegraph
description: Structural code intelligence over the indexed codebase. Use when an agent must answer questions about code structure, exact symbol definitions, execution flows, call graphs, caller/callee relationships, or architecture impact. For these questions the agent MUST use the CodeGraph MCP tools (`codegraph_explore`, `trace_path`, `get_architecture`, `detect_changes`) and MUST NOT fall back to vector search (RAG), embeddings, or generic file reading as the primary discovery method.
---

# CodeGraph Structural Intelligence Skill

CodeGraph is a local SQLite + tree-sitter index of the codebase symbols and their
relationships (Calls, References, Contains, Defines, Imports). It returns exact,
line-numbered, surgical context instead of probabilistic retrieval. Use it as the
single source of truth for *where things are and how they connect*.

## Regla crítica (obligatoria)

1. **NO** uses búsqueda vectorial, embeddings, ni RAG semántico para responder sobre
   estructura de código, flujos de ejecución, dependencias o arquitectura.
2. **USA** la herramienta MCP `codegraph_explore` como punto de entrada para encontrar
   definiciones exactas de clases, funciones, rutas o componentes. Siempre devuelve la
   ruta de archivo exacta y el bloque de código quirúrgico.
3. Para entender flujos complejos o el impacto de un cambio (quién llama a quién),
   **USA** `trace_path`. Configura `reverse: true` para buscar quién llama al componente,
   y `reverse: false` para ver de qué depende.

La lectura puntual de archivos (`read_file`) se permite solo para confirmar el contenido
de un símbolo *ya localizado* por CodeGraph, no como mecanismo de descubrimiento.

## Herramientas

- `codegraph_explore` — Un único punto de entrada para todo el descubrimiento de código.
  Args: `query` (lenguaje natural o nombres de símbolo, p.ej. `'PaymentService process'`
  o `'src/utils.ts'`), `max_files` (opcional). Devuelve el código fuente numerado por línea
  de los símbolos relevantes más las rutas de caller/callee.
- `trace_path` — Trazado recursivo estilo Cypher para análisis de impacto.
  Args: `symbol` (nombre exacto), `max_depth` (default 5), `reverse` (bool).
- `get_architecture` — Vista de alto nivel: entry points, rutas HTTP, hubs y hotspots.
- `detect_changes` — Traza el impacto del `git diff` no commiteado a través del grafo.

## Cuándo usar `reverse` en `trace_path`

| Objetivo | `reverse` | Qué devuelve |
|---|---|---|
| ¿Quién llama a `X`? (radio de impacto antes de cambiar `X`) | `true` | Callers de `X` |
| ¿Qué llama/depende `X`? (árbol de dependencias) | `false` | Callees de `X` |

## Flujo recomendado

1. **Explora** con `codegraph_explore` para localizar símbolos y su definición exacta.
2. **Traza** con `trace_path` para entender el flujo o medir el impacto del cambio.
3. **Confirma** leyendo el archivo puntual solo si necesitas contexto adicional fuera del
   bloque quirúrgico que ya devolvió `codegraph_explore`.

## Anti-patrones

- ❌ Usar `mem_search` / RAG / búsqueda semántica para "¿dónde se define X?" o "¿quién
  llama a X?". CodeGraph es determinista y exacto para eso.
- ❌ Leer archivos en cadena (`read_file` de archivo en archivo) para seguir una llamada.
  Usa `trace_path` en su lugar.
- ❌ Asumir el grafo vacío como "el símbolo no existe": si `codegraph_explore` no devuelve
  nada, primero verifica con `get_architecture`/`stats` si el proyecto está indexado; si lo
  está y aun así no aparece, entonces cae a `grep`.
- ❌ Mezclar el DB del maturity scanner (`.xavier/codegraph.sqlite`) con el DB del MCP
  (`data/code_graph.db`). El grafo estructural autoritativo es el del MCP.

## Mantenimiento

- Las herramientas MCP se declaran en `src/server/mcp/tools_code_graph.rs`.
- El engine real vive en el crate `code-graph/` (`db/`, `query/`, `indexer/`, `parser/`).
- El trazado recursivo es `code_graph::db::cypher::CodeGraphDB::trace_path`.
