# ADR-045 — Compaction opt-in and ingestion cursors

| Campo | Valor |
|-------|--------|
| **ID** | ADR-045 |
| **Estado** | Aceptado |
| **Fecha** | 2026-10-12 |
| **Autores** | Xavier architecture contributors |

## Context

PR #2978 made scheduled semantic compaction opt-in (`XAVIER_SEMANTIC_COMPACTION=1`, default off) and removed the truncation fallback in `src/memory/manager/compression.rs`. Session ingestion uses persisted cursors under `data/ingest-cursors/` (`src/memory/ingest_cursor.rs`, loop in `src/cli/server.rs`). We measured on the reference node that a cursor cycle over 4 sources took 0.14 s with `read=0` and ~3 % CPU, while a first full pass without cursors took ~67 min on one core. We need to document the decisions regarding semantic compaction and ingestion cursors.

## Decision

1. Scheduled semantic compaction is opt-in via `XAVIER_SEMANTIC_COMPACTION=1`. If the summary returned by the compactor is missing, empty, or longer than the original content, the memory is left untouched. Compaction never truncates stored content as it could overwrite stored content.
2. Periodic ingestion runs with persisted per-source cursors. Operators may set the interval using `XAVIER_INGESTION_INTERVAL_SECS`. Deleting cursors triggers a long full pass.

## Consequences

Storage growth is accepted until a verified compaction model is configured. To enable compaction safely, operators must configure a verified model and perform a backup first.
