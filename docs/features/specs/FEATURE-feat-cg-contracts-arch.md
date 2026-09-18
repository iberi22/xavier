# FEATURE: Code-Graph Contracts + Architecture Signals (O7)

**Status:** `planned` (implemented 2026-09-18, pending verify-pipeline) | **Score:** 80% | **Wave:** O7 (requiere O1, O3)

> Implementa honestamente: ciclos a nivel fichero sobre Calls (resolución de imports a módulos, diferida); `edit-check` contra HEAD, diferido al CLI (núcleo puro: fingerprint + compare).

## Overview

Contratos pre-merge (edit-check / safe-delete / verify) + señales de arquitectura (god-nodes, surprising, ciclos, rationale). Extensión opcional O8: Leiden.

## Origen (qué / de dónde / por qué)

- R5 ripwire (`COMMANDS.md --edit-check/--safe-delete/--verify`): `unchanged/new-symbol/contract-change`, `risk=` sin veredicto go/no-go, veredicto triple con evidencia.
- G7 graphify (`analyze.py`, `node-summaries-rfc.md`): god-nodes con blocklist, surprising cross-file, `import-cycles`, rationale `NOTE/WHY/HACK` + ADRs como nodos.
- G5 (opcional O8): `cluster.py` Leiden seed 42 + exclude-hubs + label-por-hub (vendorizar: sin crate estándar).
- Por qué: es el gate que falta entre "cambio propuesto" y "merge seguro", y la superficie que `graph-explorer`/`trace_path` deben mostrar primero.

## User stories

- US-113: como revisor, quiero saber si mi edit cambió un contrato y qué call-sites son incompatibles antes del push.
- US-114: como arquitecto, quiero ver god-nodes, conexiones sorprendentes y ciclos de imports del repo.

## Acceptance criteria

- [ ] `edit-check` vs HEAD con `params_was/now` + call-sites incompatibles marcados.
- [ ] `safe-delete` compone callers+impact+uses+tested y nombra `risk`, sin veredicto.
- [ ] `verify("calls(A,B)")` → `confirmed/refuted/not-established` + evidencia; cero nunca refuta.
- [ ] Rationale `NOTE/WHY` indexados como nodos linkados al código.

## Tests (TDD, escribir primero)

- `test_edit_check_contract_change_detects_arity_break`.
- `test_safe_delete_names_risk_never_verdict`.
- `test_verify_three_valued_with_evidence` (incl. cero-no-refuta).
- `test_god_nodes_exclude_builtins`.
- `test_import_cycles_found_length_bounded`.
- `test_rationale_note_linked_to_symbol`.

## REQ-IDs

- REQ-059 (nuevo).

## Implementation paths

- `code-graph/src/query/` (contratos), `code-graph/src/parser/` (rationale), `src/server/mcp/tools_core.rs`, `panel-ui/` (superficie graph-explorer), `scripts/` (hook pre-push opcional).
