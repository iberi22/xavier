# FEATURE: Code-Graph Blast Radius + Test Gate (O3)

**Status:** `planned` (implemented 2026-09-18, pending verify-pipeline) | **Score:** 80% | **Wave:** O3

> Implementa honestamente: roles 1:1 desde `EdgeType` (sin read/write inventados — el grafo no ve data-flow); obligación del gate sobre seeds descubiertos (gating de radio completo llega con baselines CLI situ).

## Overview

Blast-radius transitivo con roles + tests-to-run + gate pre-merge: de "quién llama" a "qué rompo y qué test lo cubre".

## Origen (qué / de dónde / por qué)

- R2 ripwire (`COMMANDS.md` `--impact/--uses/--affected/--situ/--test-gate`, `EVALS.md`): `reaches` transitivo, roles call/read/write/import/extends/type, `run=` solo si derivable, exit 4 si hay radio sin test.
- Por qué: `blast_radius` existe (`code-graph/src/query/mod.rs:220-283`, verificado vía `codegraph_explore` + `trace_path`: callers en `src/cli/handlers/code.rs:1337` y test `:391`) pero sin roles, sin mapeo a tests y sin gate.

## User stories

- US-105: como agente, quiero el `reaches` transitivo de mi cambio con roles para saber si es seguro modificar.
- US-106: como CI, quiero que el gate falle (exit≠0) si el cambio tiene radio sin cobertura de tests.

## Acceptance criteria

- [ ] `blast_radius` devuelve set transitivo + `radius_tested/radius_untested` + roles por sitio.
- [ ] `test-gate`: lista tests-to-run con `run=` derivable; exit 4 si obligación no vacía.
- [ ] `situ` lee `git diff` y añade co-change partners fuera del diff.

## Tests (TDD, escribir primero)

- `test_impact_transitive_two_hops` (A→B→C: cambiar C alcanza A).
- `test_uses_roles_distinguish_read_write_call`.
- `test_testgate_exit4_on_untested_radius` / exit 0 con cobertura total.
- `test_run_command_only_when_derivable` (sin `run=` inventado).
- Integración: repo fixture con cambio + gate en rojo y en verde.

## REQ-IDs

- REQ-055 (nuevo).

## Implementation paths

- `code-graph/src/query/mod.rs` (impact/uses/test-gate), `src/cli/handlers/code.rs`, `src/server/mcp/tools_core.rs` (`trace_path` enriquecido), `scripts/` (gate CI).
