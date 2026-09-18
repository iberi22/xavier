# FEATURE: Code-Graph Incremental IDs (O4)

**Status:** `planned` (implemented 2026-09-18, pending verify-pipeline) | **Score:** 80% | **Wave:** O4

## Overview

IDs multi-proyecto reales + incremental por hash de contenido + resolución de llamadas en 2 fases. Ver ADR-033.

## Origen (qué / de dónde / por qué)

- G3 graphify (`base.py#_file_stem`, `build.py#build_merge`, `cache.py`, `manifest.json`): IDs `{fullpath}_{símbolo}`, cache por hash, prune de borrados.
- G4 graphify (`symbol_resolution.py`): `raw_calls` 2 fases (intra EXTRACTED + cross INFERRED conservador + god-guard).
- Por qué: `project_id="default"` fijo bloquea multi-proyecto; `mtime` da falsos positivos; regex de 1 fase (`call_resolution.rs:229-248`) genera ruido cross-file.

## User stories

- US-107: como operador multi-instancia, quiero IDs sin colisión entre proyectos (`swal/{app}/{instance}`).
- US-108: como desarrollador, quiero re-scan tras `git pull` que solo re-extraiga lo cambiado (hash, no mtime).

## Acceptance criteria

- [ ] `project_id` real en el ID estable + tabla rewire para IDs `default` legacy (versionada).
- [ ] Sin cambios de contenido → sin re-extracción (test con toques `mtime`-only).
- [ ] Cross-file: precisión medida en fixture ≥ baseline actual, con god-guard documentado.

## Tests (TDD, escribir primero)

- `test_ids_unique_across_projects` (mismo path+símbolo, distinto proyecto).
- `test_mtime_touch_without_content_change_skips_reextract`.
- `test_deleted_file_prunes_symbols_and_edges`.
- `test_cross_file_god_guard_skips_ambiguous_hub`.
- `test_legacy_default_ids_rewired` (migración versionada).

## REQ-IDs

- REQ-056 (nuevo).

## Implementation paths

- `code-graph/src/types.rs` (IDs), `code-graph/src/db/mod.rs` (manifest+cache), `code-graph/src/indexer/{mod.rs,call_resolution.rs}` (2 fases), `src/session/` (project_id, REQ-004).
