# Extracción de ideas de fsearch → Xavier

> Créditos y trazabilidad. Xavier **no copia código** de fsearch: reimplementa en
> Rust las ideas de la tabla. Licencia y texto completo en
> [`THIRD_PARTY.md`](../THIRD_PARTY.md).

## Agradecimiento

Gracias a **Noah Dunnagan** por [fsearch](https://github.com/noahdunnagan/fsearch),
un buscador de nombres de ficheros en Rust cuyo diseño (consultas literales,
robustez frente a erratas, cursores de reindexado) nos sirvió de referencia.
Su trabajo es MIT, © 2026 Noah Dunnagan.

## Fuente

| Proyecto | Commit pineado | Licencia |
|---|---|---|
| `noahdunnagan/fsearch` | `af9476d39ec98108552670adf6badbbd77331b0a` | MIT |

Ficheros de fsearch consultados: `src/query.rs`, `src/index.rs`, `src/engine.rs`,
`src/content.rs`, `demo/vs_fff.py`.

## Tabla de extracción

Fuente: lógica de fsearch → destino en Xavier. "Antes" y "Después" son la métrica
medida; donde aún no hay medición se escribe `pendiente`.

| # | Lógica fsearch (fichero) | Dónde se usa en Xavier | Estado | Antes | Después |
|---|---|---|---|---|---|
| P0 | Metodología de medición de robustez (`demo/vs_fff.py`: % de consultas con errata que siguen encontrando el objetivo primero) | `tests/fsearch_port_metrics.rs`: corpus sintético de 40 docs en tempdir, 10 consultas por grupo (puntuación, errata, control), modos híbrido y solo texto | Iter. 1, hecho | no existía métrica de robustez de consultas | `ok_rate` / `top1_rate` / `top5_rate` por grupo; protege P1 como regresión |
| P1 | Términos literales; la puntuación no es sintaxis (`src/query.rs`) | `src/domain/cycle_breaks/w30_10.rs` (`build_fts_query`): cada término se emite como frase FTS5 entrecomillada. Lo usan la búsqueda híbrida de memorias, la de código (`src/codebase/db.rs`), las colecciones del RAG (`src/collections/store.rs`) y los logs de servicio | Iter. 1, hecho | consultas con `/ . - :` (`src/main.rs`, `sqlite-vec`, `INC-4821`, `v0.4.1`): **0/10 devuelven Ok** (error de sintaxis FTS5 que tumba la búsqueda híbrida entera); top-1 0 % | **10/10 Ok, top-1 100 %**, en modos híbrido y solo texto; control sin puntuación sin cambios (100 %). Mutation test: 4 tests rojos al retirar el fix |
| P2 | Tolerancia a una errata en palabras de 5+ letras (`src/query.rs`) | Expansión por `fts5vocab` en `temp` sobre `build_fts_query` (en filas cifradas el índice FTS solo contiene la ruta, así que ahí solo corrige erratas de rutas) | Iter. 2, pendiente | con errata (10 consultas): top-1 60 % | pendiente |
| P3 | Cursor persistido y escritura atómica: solo se reprocesa lo que cambió (`src/index.rs`, `src/engine.rs`) | Importadores `src/memory/{opencode,codex,antigravity,hermes}_importer.rs` | Iter. 2-3, pendiente | pendiente (hoy los cursores solo viven en memoria: cada arranque hace una pasada completa) | pendiente |
| P4 | Índice de trigramas y relectura del fichero fresco (`src/content.rs`) | Tabla FTS5 nueva para grep/regex de código; hoy `code_fts` usa `porter unicode61` (`src/codebase/db.rs:670-671`) | Iter. 4, opcional | pendiente | pendiente |

Cada fila se mide antes y después con el mismo harness (P0) antes de cerrarse.

## Atribución en el código

Cualquier función inspirada en fsearch lleva un comentario breve:

```rust
// Inspired by fsearch (MIT, noahdunnagan/fsearch): <lógica>
```

## Descartado y por qué

- **Single-owner / followers**: SQLite en WAL ya resuelve la concurrencia.
- **mmap de nombres, crawl paralelo y FSEvents**: no aplican; FSEvents es específico de macOS.
- **`LIKE` → rango en `sqlite_store.rs`**: almacén legado, fuera de la ruta de búsqueda principal; se evaluará aparte si hace falta.
