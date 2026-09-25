---
name: xavier-issue-creation
description: "Creación de issues delegables a agentes autónomos (Jules) para el repo Xavier. Template canónico GitCore v3.8 adaptado: micro-fragmentación, lotes ≤15, file islands disjuntos, acceptance criteria ejecutables y protocolo Crear→Verificar→Releer→Label."
version: 3.8.0-xavier
tags: [issues, jules, gitcore, wave, delegation]
---

# Xavier Issue Creation — Delegación a agentes autónomos

Propósito: crear issues que **Jules u otros agentes** puedan ejecutar sin
supervisión, y que el ledger `docs/features/features.json` pueda verificar.
Idioma: issues, PRs, commits → **siempre en inglés**.

## Regla de platino

```
Crear → Verificar → Releer → SOLO ENTONCES → label "jules"
```

1. **Plan (offline)**: hasta 15 micro-issues con *file islands* 100% disjuntos.
   Ninguna tarea puede compartir archivos modificables con otra del lote.
2. **Create (sin label jules)**: body completo en archivo, `gh issue create
   --body-file`, labels `olaN,wave-N`.
3. **Pre-dispatch verify**: `gh issue view <N> --json body` — secciones
   obligatorias presentes; script de islands sin intersecciones.
4. **Dispatch simultáneo**: `gh issue edit <N> --add-label jules` para todo el lote.

## Micro-fragmentación

- <150 líneas de cambio por tarea. Features complejas se dividen en:
  `A: tipos/contratos` → `B: lógica core` → `C: conectores (routes, MCP, CLI)` →
  `D: tests`.
- Objetivo: 10–20 min por issue, PR por issue, 1 PR = 1 feature.

## Guardas específicas de Xavier

- **Rutas absolutas desde la raíz del repo**: `src/...`, `code-graph/src/...`,
  `xavier-core/src/...`, `panel-ui/src/...`. Nunca rutas ambiguas.
- **`.gitcore/features.json` / `docs/features/features.json`**: intocable dentro
  de issues de wave — solo se reconcilia al cierre de la ola (feat reconcile
  dedicado). El ledger nunca sube de `status` a mano; lo promueve un run verde.
- **Acceptance criteria ejecutables**, siempre comandos reales:
  - `cargo fmt --all --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo check --workspace --features ci-safe`
  - `cargo test -p <crate>` o `cargo test --workspace`
  - `grep -rn "ExactPattern" src/...` ≥ 1 match
- **CI hosted no disponible** (plan gratuito, sin minutos de Actions): el juez es
  `scripts/verify-pipeline.sh` en local. Todo issue debe indicar cómo se verifica
  localmente.
- **Tokio + Rayon**: si el issue toca código paralelo, incluir la golden rule
  (`spawn_blocking`, nunca `.par_iter()` en worker Tokio).
- **Secrets/config**: si el issue añade `std::env::var`, los AC deben exigir la
  clave en `.env.example` con placeholder.

## Template (copiar al body)

```markdown
# [Ola N.XX] feat-<name> — <Title>

> Ola N — [Core|Infra|UI|Telemetry]. Labels: `olaN`, `wave-N`

## Current State (measurable)
- Feature: `feat-xyz` at N% in `docs/features/features.json`
- File: `src/path/file.rs` (N lines)
- Tests: N existing / N passing

## Desired State (delta)
- Specific addition: [struct/function/handler exacto]
- File target: `src/path/new_module.rs`

## Acceptance Criteria (command-verifiable)
- [ ] `cargo check --workspace --features ci-safe` — 0 errors
- [ ] `cargo test -p xavier` — all tests pass
- [ ] `cargo clippy --all-targets -- -D warnings` — clean
- [ ] `grep -rn "ExactStruct" src/` >= 1 match

## Files to Modify
| File | Current state | Change | Risk |
|------|---------------|--------|------|
| `src/...` | ... | ... | LOW |

## DO NOT touch
- `.gitcore/features.json` — reconciled at wave end
- `src/<otro-modulo>/` — assigned to Issue #N

## Anti-Hallucination Guard
1. READ every file listed before writing.
2. Follow existing patterns (thiserror in libs, anyhow in binaries).

## Merge order
- Within wave: [n/15] · Effort: Small (<45m) · Parallel with: all (disjoint islands)
```

## Cierre de ola

1. Integrar PRs en orden de merge number (cosechar delta antes de cerrar duplicados).
2. `cargo fmt --all` en la punta integrada si algún PR trajo diffs de formato.
3. Run `scripts/verify-pipeline.sh` → promotes statuses honestly.
4. Poda: borrar ramas merged (`git push origin --delete`), cerrar issues con link al PR.
