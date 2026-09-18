# ADR-032: Rúbrica única de confidence + honestidad etiquetada (O1)

**Status:** accepted | **Date:** 2026-09-18 | **Relates:** REQ-053, `feat-cg-honest-confidence`

## Context

Xavier tiene tres escalas de confianza incompatibles (F4): edges code-graph 0.3–1.0 (`call_resolution.rs`), belief 0.3/0.6/0.9 (`belief_graph.rs`), score entero 10/5/1 (`db/mod.rs:691-722`). Además, los conteos pueden leerse como totales cuando son pisos (dispatch dinámico, macros, callbacks invisibles al AST). Ripwire demuestra que la honestidad etiquetada (`counts_floor`, `amb`, `capped`, rehúso vs 0) y Graphify que una rúbrica discreta compartida es lo que permite fusionar señales sin calibrar cada vez.

## Decision

1. Adoptar la rúbrica Graphify como norma única: `EXTRACTED=1.0` (explícito en fuente), `INFERRED ∈ {0.95, 0.85, 0.75, 0.65, 0.55}` con la evidencia exigida por nivel, `AMBIGUOUS ∈ 0.1–0.3` (visible, nunca omitido). Mapear belief y score entero a esta escala (tabla en el spec O1).
2. Todo conteo de grafo lleva `floor` cuando no puede ser total; `0` significa "none found", jamás "none exists". Selectores desconocidos rehúsan con sugerencia en vez de devolver vacío.
3. Reescritura Rust propia; atribución a ripwire (`docs/COMMANDS.md`, `METHODOLOGY.md §6`) y graphify (`how-it-works.md#Confidence tagging`) en `THIRD_PARTY.md`.

## Consequences

- Positivo: una sola escala para RRF, `NavigationPolicy`, belief y UI; agentes sin falsos negativos silenciosos.
- Negativo: migración de valores históricos (backfill con regla documentada, no recálculo).
- Riesgo: tests actuales que asertan scores viejos deben actualizarse en la misma ola (TDD red-first los revela).
