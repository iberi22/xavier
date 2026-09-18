# FEATURE: Code-Graph Honest Confidence (O1)

**Status:** `planned` (implemented 2026-09-18, pending verify-pipeline) | **Score:** 80% | **Wave:** O1 (fundación)

## Overview

Rúbrica única de confidence + honestidad etiquetada en todas las respuestas del code-graph y la búsqueda. Ver ADR-032.

## Origen (qué / de dónde / por qué)

- R1 ripwire (`docs/COMMANDS.md` `--callers/callees/impact/uses`, `ARCHITECTURE.md §4`, `METHODOLOGY.md §6`): `counts_floor`, `amb=K`, `shown/total/capped`, rehúso con did-you-mean; `0` = "none found".
- G1 graphify (`how-it-works.md#Confidence tagging`, `extraction-spec.md`): `EXTRACTED=1.0`, `INFERRED {0.95,0.85,0.75,0.65,0.55}`, `AMBIGUOUS 0.1-0.3`.
- Por qué: 3 escalas incompatibles en Xavier + ceros silenciosos que los agentes leen como verdad.

## User stories

- US-101: como agente, quiero que todo conteo indique si es piso o total para no asumir completitud falsa.
- US-102: como desarrollador, quiero una sola escala de confidence en edges, belief y ranking con tabla de mapeo.

## Acceptance criteria

- [ ] Toda respuesta de grafo incluye `floor|total`, `shown/total/capped`, `amb` donde aplique.
- [ ] Selector desconocido rehúsa con sugerencia (no vacío silencioso).
- [ ] Tabla de mapeo belief/score→rúbrica aplicada + backfill documentado.

## Tests (TDD, escribir primero)

- `test_zero_means_none_found_not_none_exists` (query a símbolo inexistente rehúsa, no `count=0` ambiguo).
- `test_counts_carry_floor_flag` (dispatch dinámico → `floor=1`).
- `test_confidence_rubric_mapping` (belief 0.6→0.65? según tabla; score 10→1.0, 5→0.75, 1→0.55).
- `test_truncation_discloses_caps` (`shown<total` → `capped=1` + estimación).
- Doc-test del contrato de respuesta en `query/mod.rs`.

## REQ-IDs

- REQ-053 (nuevo).

## Implementation paths

- `code-graph/src/query/mod.rs` (contrato de respuesta), `code-graph/src/indexer/call_resolution.rs` (rúbrica), `src/retrieval/gating.rs` + `src/memory/belief_graph.rs` (mapeo), `src/server/mcp/tools_core.rs` (exposición).
