# [Ola Graph · 05] panel-ui multi-layer Graph explorer (Roadmap | Memory KG | Code)

> Part of **Xavier Graph Explorer** wave. User-facing shell: three layers in ConfigModal, shared canvas, live fetch with token.
>
> **Prerequisite:** Ola Graph · 03 (routes) + · 04 (adapters). Prefer base branch with PR #663 (Roadmap CRUD) merged so Roadmap save still works.

## Web Research Required (Jules must search the web)

Before implementing, search the internet for:
1. **Accessible tablist pattern** — search: `ARIA tabs tablist tabpanel WAI-ARIA`
2. **Empty states UX** — search: `empty state CTA best practices dashboard`

## Exact Technical Context

- **ConfigModal** `panel-ui/src/components/ConfigModal.tsx`:
  - Main tabs ~lines **108–168**
  - Current graph tab label should be **Roadmap** after #663; add **sub-layer** switcher **inside** graph tab: `Roadmap | Memory KG | Code`
  - Graph render ~lines **184–250** with filters for roadmap only
- **GraphView** `panel-ui/src/components/GraphView.tsx` — roadmap-specific paint (org/project colors). Options:
  A) Generalize `GraphView` to accept `CanvasGraph` + optional roadmap CRUD callbacks, OR
  B) Keep `GraphView` for roadmap; add `GraphCanvas.tsx` for memory/code read-only
  Prefer **B** if smaller diff; prefer **A** if less duplication. Pick one and document in PR.
- **App.tsx** already has `api()` with token and roadmap save after #663
- **Adapters** from issue 04: `memoryViewToCanvas`, `codeViewToCanvas`
- **Endpoints** (after issue 03):
  - `GET /memory/graph/view`
  - `GET /memory/graph/entities/{id}` (detail)
  - `GET /code/graph/view`
  - `GET /code/stats`
  - `POST /code/scan` body `{ "path": "src" }` (existing)
  - Roadmap: `GET/POST /panel/api/graph` (existing)

> CRITICAL:
> - Roadmap CRUD must keep working (do not break #663 save path).
> - Memory/Code layers are **read-only** in v1 (no POST to entity graph).
> - Filters (date/milestone) apply **only** to Roadmap layer.
> - Never fall back to Nexus Corp mock for empty roadmap (if #663 present).
> - Use **pnpm** only; no new dependencies.
> - DO NOT touch Rust.

## Problem

Users only see roadmap (or mock). Real EntityGraph and code_graph.db are invisible in the panel.

## Acceptance Criteria

- [ ] Inside graph section, layer toggle (accessible):
  - `role="tablist"` with tabs Roadmap / Memory KG / Code
  - `aria-selected` on active tab
- [ ] **Roadmap layer**: existing GraphView + filters + empty CTA (from #663)
- [ ] **Memory KG layer**:
  - On select: `GET /memory/graph/view` with token
  - Loading + error + empty (“No entities yet — add memories”) states
  - Render canvas via adapter
  - Click node → optional detail panel showing kind/trust/memory_count (from node.meta or second fetch)
  - Show `truncated` badge if API says truncated
- [ ] **Code layer**:
  - On select: `GET /code/stats`; if `total_symbols===0`, CTA button **Scan codebase** → `POST /code/scan` `{path:"src"}` then reload view
  - Else `GET /code/graph/view?mode=overview`
  - Double-click or “Expand” on node → `GET /code/graph/view?mode=ego&query=<id or label>`
  - Empty/error/loading states
- [ ] Keyboard: layer tabs focusable; icon-only buttons have `aria-label`
- [ ] Vitest or Playwright: at least one test that mocks the three GETs and asserts tab switch shows expected text/empty/canvas
- [ ] `cd panel-ui && pnpm test && pnpm run typecheck` pass
- [ ] Update dashboard e2e selectors if they still say wrong tab names

## Files to Modify

| File | Change |
|---|---|
| `panel-ui/src/components/ConfigModal.tsx` | Layer tabs + fetch orchestration |
| `panel-ui/src/components/GraphView.tsx` and/or `GraphCanvas.tsx` (NEW) | Shared or split canvas |
| `panel-ui/src/App.tsx` | Only if token/api helpers need lift |
| `panel-ui/tests/*` | Layer switch + mocks |
| `panel-ui/src/api/graphAdapters.ts` | Only if small fixes needed |

**DO NOT touch:** Rust sources, Cargo.toml, `xavier-core/`

**NEVER create root loose files.**

## Verification

```bash
cd panel-ui
pnpm test
pnpm run typecheck
pnpm run build
```

## Dependencies and Merge Order

- **Depends on:** Ola Graph · 03 (routes live), · 04 (adapters); ideally #663 merged
- **Can run in parallel with:** Ola Graph · 06 (different files)
- **Must merge before:** Ola Graph · 07
