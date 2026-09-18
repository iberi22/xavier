# FEATURE: Code-Graph Language Registry (O2)

**Status:** `planned` (implemented 2026-09-18, pending verify-pipeline) | **Score:** 80% | **Wave:** O2

## Overview

`LanguageConfig` unificada + registry de resolvers por sufijo para añadir lenguajes sin tocar el núcleo del indexer.

## Origen (qué / de dónde / por qué)

- G2 graphify (`extractors/models.py#LanguageConfig`, `extractors/engine.py`, `resolver_registry.py`, `ARCHITECTURE.md#Adding`): config por lenguaje (gramática, tipos de nodo, accessors, import-handler, sanitizer) + registro ordenado con warn-and-continue.
- Por qué: el dispatch de Xavier está disperso (`Language::from_extension` en `types.rs:37-61`, `chain_for` en `parser/mod.rs`) y cada lenguaje nuevo toca el núcleo.

## User stories

- US-103: como mantenedor, quiero añadir un lenguaje registrando un `LanguageConfig` + tests sin modificar `indexer`.
- US-104: como agente, quiero que un lenguaje sin gramática se reporte como `unindexed` (honestidad O1), no como vacío.

## Acceptance criteria

- [ ] `LanguageConfig` cubre: extensiones, queries tree-sitter, tipos de símbolo, import-handler, builtins a ignorar (anti-god-nodes, cf. `base.py#_LANGUAGE_BUILTIN_GLOBALS`).
- [ ] Añadir lenguaje = 1 struct + 1 fixture + tests (verificado borrando un lenguaje y re-registrándolo solo con eso).
- [ ] Fallback `unindexed="ext:N"` en el header del mapa.

## Tests (TDD, escribir primero)

- `test_register_language_without_touching_core` (nuevo lenguaje dummy solo vía registro).
- `test_builtin_globals_excluded_from_hubs` (String/Vec/print no son god-nodes).
- `test_unindexed_language_reported_not_empty`.
- `test_resolver_warn_and_continue` (resolver que falla no tumba el scan).

## REQ-IDs

- REQ-054 (nuevo).

## Implementation paths

- `code-graph/src/parser/mod.rs`, `code-graph/src/types.rs`, `code-graph/parsers/`, `queries/` (ampliación tipo `tags.scm` por lenguaje, ref. R6 ripwire).
