# FEATURE: Code-Graph Budget Query (O5)

**Status:** `planned` (implemented 2026-09-18, pending verify-pipeline) | **Score:** 80% | **Wave:** O5

## Overview

Presupuesto de tokens con gate + query barata previa al vector (expansión por vocabulario, BFS/DFS acotados).

## Origen (qué / de dónde / por qué)

- R3 ripwire (`ARCHITECTURE.md §rank/serialize`, `COMMANDS.md --pack-task/--token-budget`): `bundle=compact`, shape vs gate, cuotas por sección con roll-forward.
- G6 graphify (`skills/opencode+claude/references/query.md`, `paths.py`): expansión ≤12 tokens solo-vocab (0→parar), BFS prof.3 vs DFS prof.6, `--budget`.
- Por qué: XCP y `ContextBudgetConfig` existen pero sin gate en code; `traverse d≤8` no limita vocabulario ni presupuesto.

## User stories

- US-109: como agente con presupuesto, quiero respuestas compactas (firmas, no cuerpos) con `est_tokens` declarado.
- US-110: como agente, quiero que mi pregunta se expanda contra el vocabulario real del repo antes de gastar vector.

## Acceptance criteria

- [ ] `bundle=compact` por defecto en orientación; cuerpos solo bajo demanda (`--expand`).
- [ ] Gate de presupuesto con exit documentado; toda respuesta declara `est_tokens` calibrado por lenguaje.
- [ ] Query con 0 tokens de vocab → parada explícita (no ruido).

## Tests (TDD, escribir primero)

- `test_compact_bundle_has_no_bodies` + `est_tokens` presente.
- `test_budget_gate_triggers_over_limit` (exit + `over_ceiling`, sin truncado silencioso).
- `test_vocab_expansion_empty_query_stops`.
- `test_bfs_depth3_vs_dfs_depth6_semantics` en fixture.
- Proptest: `shown+withheld == total` bajo cualquier presupuesto.

## REQ-IDs

- REQ-057 (nuevo).

## Implementation paths

- `src/memory/pack.rs`, `src/context/orchestrator.rs`, `code-graph/src/query/` (bundle+vocab+BFS/DFS), `src/server/mcp/tools_core.rs`.
