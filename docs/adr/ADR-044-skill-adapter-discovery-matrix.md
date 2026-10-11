# ADR-044 — Skill adapter discovery matrix

| Campo | Valor |
|-------|--------|
| **ID** | ADR-044 |
| **Estado** | Propuesto |
| **Fecha** | 2026-10-11 |
| **Autores** | Xavier architecture contributors |
| **Relacionados** | ADR-034 |

## Context

Design `docs/design/skill-controller/01-ARCHITECTURE-AND-ADR.md` §Publication strategy proposes one symlink per selected skill. We need to measure compatibility of this symlink projection strategy across different CLI adapters (Hermes, OpenClaw, Gemini, Codex, Claude Code, OpenCode, and agy). We need to verify if symlinks are correctly discovered by these adapters, and if a copy-fallback rule is necessary.

## Opciones que compiten

| Opción | Descripción | Coste/complejidad esperada |
|--------|-------------|----------------------------|
| A — Symlink default with explicit copy-fallback | Use symlinks as the primary mechanism; allow copy projection only for adapters that fail the symlink test. | Low cost; high security; ensures only needed exceptions use copies. |
| B — Always copy | Always use regular file copies for skill deployment. | High cost; pollutes workspaces; complicates synchronization. |

## Simulación (OBLIGATORIO)

**Recorded decision: accept on measured compatibility matrices.** The decision is based on empirical compatibility testing rather than multi-scenario modeling.

- **Motor:** `XAVIER_ADAPTER_LIVE=1` test suite
- **Runs / seed:** Not applicable
- **Resultado:** See compatibility matrix in Decision.
- **¿Coinciden?** Sí
- **Sensitivity:** Sí
- **Reporte:** Recorded inline in this ADR.

## Supuestos y su anclaje

| Parámetro | Valor usado | Fuente |
|-----------|-------------|--------|
| Measurement Environment | isolated HOME, Xavier not running | ASSUMPTION |
| Authentication | Some adapters require authenticated sessions | ASSUMPTION |

## Decision

Symlink projection is the default for every adapter. A copy fallback is allowed only for an adapter with a recorded failing symlink test.

### Skill Adapter Discovery Matrix

Measurements are taken in an isolated HOME with Xavier not running (2026-10-11).

| Adapter | Global Root | Project Root | Measurement Date | Result |
|---------|-------------|--------------|------------------|--------|
| Hermes | Yes | Yes | 2026-10-11 | `measured` (symlinked packages discovered, unprojected package absent, regular-copy control discovered) |
| OpenClaw | Yes | Yes | 2026-10-11 | `measured` (symlinked packages discovered, unprojected package absent, regular-copy control discovered) |
| Gemini | Unknown | Unknown | 2026-10-11 | `not installed` (Gemini CLI not installed) |
| Codex | Unknown | Yes | 2026-10-11 | `pending` (needs an authenticated session) |
| Claude Code | Unknown | Yes | 2026-10-11 | `pending` (needs an authenticated session) |
| OpenCode | Unknown | Yes | 2026-10-11 | `pending` (needs an authenticated session) |
| agy | Unknown | Yes | 2026-10-11 | `pending` (needs an authenticated session) |

### Re-running measurements

Measurements are re-run using the `XAVIER_ADAPTER_LIVE=1` opt-in. Unmeasured rows block enabling that adapter.

## Consequences

- **Positivas:** Secure and standardized deployment of skills using symlinks by default.
- **Negativas / coste:** Some adapters may require a copy fallback, complicating their specific implementation. Requiring live tests slows down enabling new adapters.
- **Qué invalidaría esta decisión:** If symlinks are broadly unsupported across most major adapters, forcing a default to copy projection.

## Verificación posterior

The test suite will fail to enable an adapter if its row remains `pending` or `not installed`. Re-run tests periodically with `XAVIER_ADAPTER_LIVE=1` to update the matrix.