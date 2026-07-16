# Architectural Decision Records (ADR)

Este directorio contiene el registro de las decisiones arquitectónicas significativas tomadas durante el desarrollo de Xavier. Seguimos el formato ADR para mantener un historial transparente de *por qué* se tomaron ciertas decisiones técnicas.

## Índice de ADRs

| ID | Título | Estado | Fecha |
|----|--------|--------|-------|
| [001](./001-memory-domain.md) | QmdMemory como dominio central | ACCEPTED | 2026-04-25 |
| [002](./002-ports-when.md) | Cuándo crear un port | ACCEPTED | 2026-04-25 |
| [003](./003-agent-state.md) | Estado compartido — statics vs CliState | ACCEPTED | 2026-04-25 |
| [004](./004-cortex-plugin.md) | Cortex Enterprise Cloud Plugin | PROPOSED | 2026-04-25 |
| [005](./005-multi-crate-migration.md) | Multi-Crate Workspace Migration | PROPOSED | 2026-04-26 |
| [006](./006-vector-store-local-sqlite-vec.md) | 100% Local Vector Store with SQLite-Vec | ACCEPTED | 2026-05-10 |

## Formato
Cada ADR describe un problema específico, el contexto, la decisión tomada, las alternativas consideradas y las consecuencias (positivas y negativas) de dicha decisión.
