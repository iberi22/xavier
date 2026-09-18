# FEATURE: Code-Graph PageRank Router (O6)

**Status:** `planned` (implemented 2026-09-18, pending verify-pipeline) | **Score:** 80% | **Wave:** O6 (requiere O1)

## Overview

Ranking estructural (peso de arista + PageRank) + router léxico BM25/exacto con confidence-margin. Requiere `petgraph` como dependencia directa.

## Origen (qué / de dónde / por qué)

- R4 ripwire (`ARCHITECTURE.md §1-3` CSR/in-edges, `COMMANDS.md --rank-by/--adaptive/--map-diff`): peso `(media conf)·√nref cap8`, PageRank α=.85, teleport β=.7 a cambiados, BM25 k1=1.5 b=.75, `confidence/margin_pct` con corte adaptativo.
- Por qué: RRF fusiona pero no rankea estructura; sin PageRank no hay "qué es central"; sin router con margin no hay medida de ambigüedad del ranking.

## User stories

- US-111: como agente, quiero los símbolos rankeados por centralidad real, no solo por match léxico.
- US-112: como agente, quiero saber si el ranking es concluyente (`confidence`) o un punto de partida (`low`).

## Acceptance criteria

- [ ] PageRank iterativo determinista (mismo bytes, misma respuesta) sobre el grafo de llamadas.
- [ ] Router `exacto→BM25→subtoken` con `confidence/margin_pct` y corte adaptativo documentado.
- [ ] `--map-diff`: teleport a ficheros cambiados re-rankea sin reindexar.

## Tests (TDD, escribir primero)

- `test_pagerank_deterministic_bytes` (doble run, mismo orden).
- `test_edge_weight_formula` (media conf × √nref, cap8, sin self-loops).
- `test_router_confidence_margin` (salto relativo mayor define head; flat → `low`).
- `test_map_diff_teleport_reranks` (cambio de ficheros altera top sin reindexar).
- Bench: índice llvm-escala no requerido; fixture mediano con techo de latencia.

## REQ-IDs

- REQ-058 (nuevo).

## Implementation paths

- `code-graph/src/` (nuevo módulo `rank.rs`), `Cargo.toml` (+`petgraph`), `code-graph/src/db/mod.rs` (reemplaza score 10/5/1), `src/retrieval/gating.rs` (fusión con pesos existentes).
