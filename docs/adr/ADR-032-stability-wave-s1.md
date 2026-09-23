# ADR-032 — Estrategia de corrección de la tormenta de embeddings (Wave S1)

| Campo | Valor |
|-------|--------|
| **ID** | ADR-032 |
| **Estado** | Aceptado (implementado en v0.2.6, medido en producción) |
| **Fecha** | 2026-09-23 |
| **Autores** | Hermes (agente de estabilidad) + Jules Wave S1 (13 issues) |
| **Relacionados** | #2476 #2477 #2478 #2493-#2498 #2505, issue #2479, `.hermes/prompts/xavier-stability-agent.md` |

## Contexto

2026-09-22: el ingestion loop (`src/cli/server.rs`, ciclo 600 s) reimportaba ~19.4k registros Hermes + corpora Antigravity/OpenCode/Codex por ciclo, re-embebiendo TODO sin comparación (medido: 439-502 emb/min sostenidos, hit_rate 41%, GPU junction 63→103 °C). Mitigación previa: caché persistente 80k (drop-in, sin rebuild). La mitigación deja el coste en "un re-embed por ciclo" en vez de cero; este ADR decide la corrección estructural.

## Opciones que compiten

| Opción | Descripción | Coste/complejidad esperada |
|--------|-------------|----------------------------|
| A | Skip incremental por contenido (stable_id + `store.get`, re-embeber solo cambios) en los 4 importers | Bajo: ~15 líneas por importer + test con CountingEmbedder; fail-open ante error del store |
| B | Solo agrandar caché / alargar intervalo (config, sin código) | Mínimo a corto plazo, pero el coste por ciclo sigue siendo O(corpus) y cualquier caché fría recrea la tormenta |
| C | Deshabilitar el ingestion loop / importers (intervalo 0) | Nulo en GPU, máximo en producto: la memoria deja de aprender sesiones |

## Simulación

Motor canónico `periferia/swal-sim/adr_sim.py` (`--list` 2026-09-23): **no existe modelo aplicable** (modelos disponibles: tiers, ledger, supply, karma, council — ninguno de estabilidad/ingestión).

Decisión por evidencia medida de producción (clase de evidencia superior a Monte Carlo para un defecto determinista — las opciones difieren en especie, no en distribución):

| Parámetro | Valor usado | Fuente |
|-----------|-------------|--------|
| emb/min pre-fix | 439-502 sostenidos | `journalctl -u ollama` 2026-09-22 + `diagnostics/2026-09-22-xavier-stability-fix` (Xavier) |
| emb/min post caché-caliente | 4 | misma medición, 2026-09-23 |
| hit_rate pre → post | 41% → 88.4% (12.351 entries) | `GET /health` 2026-09-22/23 |
| coste 2º pase idéntico (A) | 0 encodes | `test_*_second_pass_no_reencode` ×6 + `tests/stability_ingestion_tests.rs` 5/5 |
| defecto real extra hallado por test S1.11 | 1 re-embed por restart (lookup no abría la DB) | `persist_roundtrip_survives_reopen` 51-vs-50 sin fix |
| regressions | 0 | `memory::` 301/301, `embedding::cache` 15/15, clippy `-D warnings`, CI verde en 9 PRs |

- **Resultado:** winner `A` (composite: elimina el coste por ciclo + cubre caché fría + conserva producto). Runner-up `B` como complemento (ya aplicado vía drop-in), `C` queda como freno de emergencia (`XAVIER_INGESTION_INTERVAL_SECS=0`).
- **Sensitivity:** la decisión no flipa por escenario — A domina a B en todo escenario con contenido repetido (>95% de los ciclos medidos: 19.370→19.393 registros con segundos de diferencia real).

## Decisión

Se adopta **A como corrección estructural** (4 importers con skip + tests), **B como mitigación operativa** (caché 80k persistente + intervalo/tuning por env), **C como freno de emergencia** (no como estado normal).

## Consecuencias

- **Positivas:** coste GPU por ciclo → solo contenido genuinamente nuevo; tormentas imposibles por diseño aunque la caché se pierda; intervalo y cuota tunables sin rebuild.
- **Negativas / coste:** `store.get` extra por registro (lectura local, despreciable frente a un embed); `bus_quota` sigue siendo aproximado bajo raza (documentado, fail-safe).
- **Qué invalidaría esta decisión** (qué dato la revierte): un ciclo donde >50% del corpus cambie de contenido real de forma sostenida (el skip dejaría de ahorrar) o `store.get` más lento que `encode` (físicamente imposible en este stack).

## Verificación posterior

- T1/T2 del agente de estabilidad (`scripts/xavier-stability-watch.sh`): <60 emb/min fuera de ciclo, hit_rate ≥90% tras calentamiento. Revisión: 2026-09-30.
- Issue #2479 (FTS5) sigue abierto y es independiente de esta decisión.
