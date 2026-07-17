# [Ola 4 · 06] Docs: guía hot-swap Ollama + métricas en USER_GUIDE_LOCAL

> Parte de **Xavier 100% Local — Ola 4**. Docs only.

## Web Research Required

1. Skim Ollama docs for accurate CLI examples: https://github.com/ollama/ollama
2. Match tone/structure of existing `docs/USER_GUIDE_LOCAL.md`

## Exact Technical Context

- Update `docs/USER_GUIDE_LOCAL.md` (created Ola 3)
- Update `docs/ROADMAP_LOCAL_FIRST.md` Ola 4 section to "IN PROGRESS" if needed
- Reference endpoints:
  - `GET /v1/ollama/models`, `POST /v1/ollama/pull`, `POST /v1/ollama/active`
  - `GET /v1/account/usage` fields
- Optional short section in `docs/LOCAL_SETUP.md`

## Problem

Users lack documentation for new Ola 4 control-plane features.

## Acceptance Criteria

- [ ] USER_GUIDE_LOCAL.md section "Gestión de modelos Ollama (hot-swap)"
- [ ] Section "Métricas de uso" with curl examples for `/v1/account/usage`
- [ ] Note: set-active updates process env; no full restart required for next chat
- [ ] No Rust code changes
- [ ] Valid markdown only

## Files to Modify

| File | Change |
|---|---|
| `docs/USER_GUIDE_LOCAL.md` | New sections |
| `docs/ROADMAP_LOCAL_FIRST.md` | Ola 4 status line only if needed |

**DO NOT touch:** `src/`, `panel-ui/`

## Dependencies and Merge Order

- **Depends on:** ideally after 02 (API names stable) — can draft early
- **Can run in parallel with:** all
- **Not last** — EPIC 07 is last
