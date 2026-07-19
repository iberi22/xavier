# [Ola Graph · 07] Docs alignment + e2e smoke + EPIC close for Graph Explorer

> Part of **Xavier Graph Explorer** wave. **LAST** issue — only after 01–06 merged (or clearly waived). Align docs with real routes; close the wave.

## Web Research Required (Jules must search the web)

Before implementing, search the internet for:
1. **API doc accuracy** — search: `OpenAPI documentation drift problems` (why docs must match mounted routes)
2. No new external crates; follow existing docs style in `docs/CORTEX_USAGE_GUIDE.md`

## Exact Technical Context

- **Docs claiming phantom routes** (must fix or mark deprecated):
  - `docs/CORTEX_USAGE_GUIDE.md` ~lines **109–127**, **305–361**: `POST /memory/entity`, `POST /memory/relation`, `POST /memory/kg/traverse`
  - `docs/CORTEX_QUICK_REFERENCE.md` Knowledge Graph section
  - `docs/public/API.md` if present with `/memory/graph`
  - `docs/issues/PHASE-3-memory-graph.md` historical — add header “superseded by Ola Graph” rather than rewrite entire design
- **Real routes after this wave**:
  - Roadmap: `GET/POST /panel/api/graph`
  - Memory: `GET /memory/graph/view`, `/memory/graph/entities`, `/memory/graph/entities/{id}`, `/memory/graph/relations`
  - Code: `GET /code/graph/view` (+ existing `/code/*`)
- **Panel UI**: ConfigModal layers Roadmap | Memory KG | Code
- **features.json**: optional note under a graph-related feature if appropriate — do not fake 100% without code merged

> CRITICAL:
> - Do **not** invent new endpoints in docs that code does not have.
> - Prefer updating docs to **real** GET APIs rather than implementing all legacy POST paths unless trivial aliases.
> - DO NOT large-refactor product code; docs + light e2e only.
> - NEVER create root `.patch` files.

## Problem

Documentation promises KG APIs and UI behavior that do not match the server/UI. After Ola Graph merges, docs and a smoke e2e must prove the feature is real.

## Acceptance Criteria

- [ ] Update `docs/CORTEX_USAGE_GUIDE.md` Knowledge Graph section with **real** curl/PowerShell examples for:
  - `GET /memory/graph/view`
  - `GET /memory/graph/entities?q=`
  - `GET /code/graph/view?mode=overview`
  - `GET/POST /panel/api/graph` (roadmap)
- [ ] Mark obsolete `POST /memory/entity` examples as **removed / not implemented** OR implement thin aliases only if already trivial — **default: document removal**
- [ ] Update `docs/CORTEX_QUICK_REFERENCE.md` similarly
- [ ] Add short `docs/GRAPH_LAYERS.md` (NEW, ≤150 lines): three layers diagram + when to use each
- [ ] panel-ui e2e or unit: mock three layers and assert tab labels + no “Nexus Corp” default (if #663 merged)
- [ ] Wave summary in `.gitcore/issues/graph-ui/PROGRESS.md` status table → Done with PR numbers
- [ ] PR description lists all Ola Graph PRs merged and residual risks (belief graph still out of scope)

## Files to Modify

| File | Change |
|---|---|
| `docs/CORTEX_USAGE_GUIDE.md` | Real KG API |
| `docs/CORTEX_QUICK_REFERENCE.md` | Real routes |
| `docs/GRAPH_LAYERS.md` (NEW) | Layer overview |
| `docs/issues/PHASE-3-memory-graph.md` | Superseded banner |
| `.gitcore/issues/graph-ui/PROGRESS.md` | Mark done |
| `panel-ui/tests/*` | Optional smoke |

**DO NOT touch:** core EntityGraph logic, code-graph crate internals, `xavier-core/`

## Verification

```bash
# Docs only: no cargo required if pure docs, but if e2e touched:
cd panel-ui && pnpm test
```

## Dependencies and Merge Order

- **Depends on:** Ola Graph · 05 and · 06 (feature complete)
- **Can run in parallel with:** nothing (closeout)
- **Must merge last**
