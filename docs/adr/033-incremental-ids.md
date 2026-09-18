# ADR-033: IDs multi-proyecto + incremental por hash de contenido (O4)

**Status:** accepted | **Date:** 2026-09-18 | **Relates:** REQ-056, `feat-cg-incremental-ids`

## Context

Los `stable_id` v2 (`project|file|name|kind|parent|signature`, `code-graph/src/types.rs:284-308`) excluyen líneas (bien) pero `project_id` está fijo a `"default"` (`indexer/mod.rs:637-643`), lo que bloquea multi-proyecto sin colisiones. El incremental usa `mtime` + delta git (`indexer/mod.rs:173-373`): barato pero con falsos positivos (toques sin cambio). Graphify resuelve ambos con IDs `{fullpath}_{símbolo}` + `manifest.json` + cache por hash de contenido + `build_merge` con prune.

## Decision

1. `project_id` real por workspace (`swal/{app_id}/{instance_id}`, REQ-004) como primer segmento del ID estable; migración con tabla de rewire para IDs `default` existentes.
2. Incremental por hash de contenido (manifest + cache), `mtime` solo como fast-path negativo; prune de símbolos de ficheros borrados + reparse de callers (ya existe el reparse 1-hop, se conserva).
3. Resolución de llamadas en 2 fases (intra EXTRACTED, cross INFERRED conservador + god-guard), reemplazando el regex de 1 fase (`call_resolution.rs:229-248`).

## Consequences

- Positivo: reindex solo-diff real, multi-proyecto sin colisiones, menos falsos positivos cross-file.
- Negativo: costo de hash en primer scan (amortizado); migración de `code_graph.db` existentes.
- Riesgo: cambio de IDs invalida cachés externas (MCP `trace_path` incluido) — versionar esquema de IDs.
