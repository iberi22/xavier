# PLAN — Xavier WAVE-5 (10 issues) — Preparado 2026-09-01

> **Estado:** PREFLIGHT READY ✅ — repo alineado 0.0.1, awaiting detalles de los 10 issues del usuario
> **Preflight:** `swal-preflight preflight --wave 10` → READY (90 turns / 500, $1.90, GH 5000/5000)
> **Version:** 0.0.1 (todos los manifests alineados) — proximo bump a 0.1.0 tras esta ola
> **GitCore:** 52/52 stable (WAVE-4), SRS 43 REQs

## Contexto del revert

- Tag `v1.0.0` prematuro (WAVE-4 100% sin QA) **eliminado** (local+remote+GH release).
- Unico tag canonico: `v0.0.1` (Initial Public Release 2026-08-30) → commit `cf2e604f`.
- Todos los manifests alineados a `0.0.1` (Cargo.toml, package.json, panel-ui, tauri.conf, code-graph).
- CHANGELOG con `[Unreleased]` listo para acumular esta ola.
- GH releases: solo `v0.0.1 Latest` (draft duplicado limpiado).

## Sistema de versiones (nuevo)

- **Canon:** `~/proyectosSWAL/docs/SWAL/VERSIONING.md` (semver 2.0.0, 0.y.z gate para 1.0.0, conventional commits)
- **Mirror Xavier:** `docs/SWAL_VERSIONING.md`
- **Tool:** `@swal/preflight` CLI (`~/proyectosSWAL/periferia/swal-preflight` + skill `~/.hermes/skills/swal-preflight`)
  - `swal-preflight check` — valida sync
  - `swal-preflight bump --to 0.1.0` — alinea manifests
  - `swal-preflight release --tag v0.1.0 --push` — tag + release
  - `swal-preflight preflight --wave N` — escanea providers, estima coste, enruta

## Recursos disponibles (scan 2026-09-01)

- **Hermes/muse-spark-1.2** (opencode-go, 1M ctx, 500 turns) — primary, coste bajo
- **agy 1.1.22** (Gemini) — research/web, paralelizable
- **GH API** 5000/5000 — holgado para 10 issues
- **Xavier :8006** — degraded/unhealthy (DB integrity false, embedding healthy) — revisable pre-wave
- **Toolchain:** cargo 1.95, pnpm 11.24, node 22.23, gh 2.92, rg 14.1, uv
- **Estimacion 10 issues:** 380k tokens, 90 turns, $1.90 → cabe en 1 sesion (500 turns)

## Enrutamiento recomendado

- Implementacion codigo/tests → hermes/muse-spark (opencode-go)
- Research web/codebase → agy o hermes research mode
- Issues/PRs → gh CLI
- Memoria/contexto → Xavier

## Preparacion completada

- [x] Revert v1.0.0 + alinear manifests + push
- [x] Crear VERSIONING.md canon + mirror
- [x] Crear CLI @swal/preflight (check/bump/release/preflight) + skill Hermes
- [x] Fix CHANGELOG [Unreleased] + .gitignore (.atl, skill-registry, .env.local)
- [x] Commit panel fixes + skill registry snippets (AGENTS.md)
- [x] Preflight READY (clean tree, versions ok)

## Siguiente paso — awaiting input usuario

Usuario dijo: “ya te digo todo” para los 10 issues. No crear issues aun.

Cuando usuario de detalles, ejecutar:

```bash
swal-preflight preflight --wave 10 --cwd ~/proyectosSWAL/apps/xavier  # re-validar
swal-preflight check --cwd ~/proyectosSWAL/apps/xavier
gh issue list --repo iberi22/xavier --limit 20 --json number,title,state,labels
# Crear 10 issues con template canonico (feat- + 11 secciones Rust, ver skill xavier-jules-wave)
# Delegar segun routing: implementacion → hermes/muse-spark, research → agy
```

## Notas para QA gate 1.0.0

No taguear 1.0.0 hasta:
- cargo test/clippy/fmt/gitleaks verde
- QA manual (happy path + degradacion)
- SRS REQs verified + docs actualizadas
- Deploy prod + health ok
- ADR firmada por BELA

Proximo version tras WAVE-5: `0.1.0` (minor, feat desde 0.0.1).
