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

| Adapter | CLI version (2026-10-12) | Global root | Project root | Offline projection test (runs in CI) | Native discovery | Reason | Test name |
|---------|--------------------------|-------------|--------------|--------------------------------------|------------------|--------|-----------|
| Codex | 0.162.1 | `$HOME/.agents/skills` | `<project>/.agents/skills` | yes | `verified locally with --ignored` | not in CI (needs the CLI) | `codex_native_startup_global_and_project_catalog` |
| Claude Code | 2.1.296 | `$HOME/.claude/skills` | `<project>/.claude/skills` | yes | `unsupported` | no offline catalog command; authenticated calls are not hermetic | `claude_native_startup_unsupported` |
| OpenCode | 2.0.27 | `$HOME/.config/opencode/skills` | `<project>/.agents/skills`, `<project>/.claude/skills` | yes | `unsupported` | no offline catalog command (opencode debug skill removed in v2) | `opencode_native_startup_unsupported` |
| agy | 1.3.3 (Gemini 3.8 Flash models) | `$HOME/.gemini/config/skills` | `<project>/.agents/skills` | yes | `unsupported` | no offline catalog command; interactive calls require Google auth | `agy_native_startup_unsupported` |
| Gemini CLI | — | `$HOME/.gemini/skills` | `<project>/.agents/skills` | yes | `unavailable` | standalone gemini CLI not installed; Gemini models are covered through agy (owner decision 2026-10-12) | `gemini_native_startup_global_and_project_catalog` |
| Hermes | 0.21.5 | `$HOME/.hermes/skills` | `<project>/.agents/skills` | yes | `unsupported` | offline startup tries to download dependencies | `hermes_native_startup_unsupported` |
| OpenClaw | 2026.9.6 | `$HOME/.openclaw/skills` | `<project>/.agents/skills` | yes | `verified locally with --ignored` | not in CI (needs the CLI) | `openclaw_native_startup_global_catalog` |

### Re-running measurements

Measurements are re-run using the `XAVIER_ADAPTER_LIVE=1` opt-in. Unmeasured rows block enabling that adapter.

## Consequences

- **Positivas:** Secure and standardized deployment of skills using symlinks by default.
- **Negativas / coste:** Some adapters may require a copy fallback, complicating their specific implementation. Requiring live tests slows down enabling new adapters.
- **Qué invalidaría esta decisión:** If symlinks are broadly unsupported across most major adapters, forcing a default to copy projection.

## Verificación posterior

The test suite will fail to enable an adapter if its row remains `pending` or `not installed`. Re-run tests periodically with `XAVIER_ADAPTER_LIVE=1` to update the matrix.