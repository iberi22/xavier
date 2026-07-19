# [Ola Graph · 04] panel-ui: shared canvas types + graph adapters

> Part of **Xavier Graph Explorer** wave. Frontend-only foundation: types and pure adapters mapping API JSON → force-graph data. No ConfigModal wiring yet (issue 05).

## Web Research Required (Jules must search the web)

Before implementing, search the internet for:
1. **react-force-graph-2d data shape** — search: `react-force-graph-2d graphData nodes links`
2. TypeScript discriminated unions for UI layers — search: `typescript discriminated union layer type pattern`

## Exact Technical Context

- **Existing roadmap types**: `panel-ui/src/types.ts` lines **53–84** (`GraphNode`, `GraphLink`, `GraphData`, `BackendGraphData`)
- **Existing roadmap helpers** (from PR #663 if merged; if not present yet, create alongside):
  - `panel-ui/src/utils/roadmapGraph.ts` — `normalizeGraphData`, `mergeFilteredGraphUpdate`, `EMPTY_ROADMAP_GRAPH`
- **API client pattern**: `getApiUrl` / `fetch` with `X-Xavier-Token` in `panel-ui/src/App.tsx`
- **Force graph consumer**: `panel-ui/src/components/GraphView.tsx` currently expects roadmap `GraphNode.type` union only
- **Do NOT rewrite GraphView fully here** — only types + adapters + tests

> CRITICAL:
> - Use **pnpm** only (never npm/yarn).
> - No new npm dependencies.
> - DO NOT touch Rust files.
> - Keep roadmap types intact (other code depends on them).

## Problem

Three backend layers return different shapes. Without a shared `CanvasGraph` + adapters, issue 05 will sprawl mapping logic inside components.

## Acceptance Criteria

- [ ] Add `panel-ui/src/types/graphLayers.ts` (or extend `types.ts` carefully):

```ts
export type GraphLayer = "roadmap" | "memory" | "code";

export interface CanvasNode {
  id: string;
  label: string;
  kind: string;
  description?: string;
  meta?: Record<string, unknown>;
}

export interface CanvasLink {
  source: string;
  target: string;
  relation: string;
  weight?: number;
}

export interface CanvasGraph {
  layer: GraphLayer;
  nodes: CanvasNode[];
  links: CanvasLink[];
  truncated?: boolean;
  stats?: Record<string, number | string>;
}
```

- [ ] Add `panel-ui/src/api/graphAdapters.ts`:
  - `roadmapToCanvas(data: GraphData): CanvasGraph`
  - `memoryViewToCanvas(json: unknown): CanvasGraph` — tolerant of missing fields
  - `codeViewToCanvas(json: unknown): CanvasGraph`
  - Normalize link endpoints to **strings** always

- [ ] Vitest unit tests in `panel-ui/tests/graphAdapters.test.ts`:
  - roadmap mapping preserves ids
  - memory view sample JSON → nodes/links
  - code view sample JSON → nodes/links
  - object `{id}` link endpoints coerced to string

- [ ] `cd panel-ui && pnpm test && pnpm run typecheck` pass
- [ ] Diff only panel-ui files listed

## Files to Modify

| File | Change |
|---|---|
| `panel-ui/src/types/graphLayers.ts` (NEW) | Canvas types |
| `panel-ui/src/api/graphAdapters.ts` (NEW) | Pure adapters |
| `panel-ui/tests/graphAdapters.test.ts` (NEW) | Unit tests |

**DO NOT touch:** Rust, `ConfigModal.tsx`, `GraphView.tsx` (except if export type re-export is needed — prefer not), `package.json` deps

**NEVER create root loose files.**

## Verification

```bash
cd panel-ui
pnpm test
pnpm run typecheck
```

## Dependencies and Merge Order

- **Depends on:** nothing (can use fixture JSON matching planned API)
- **Can run in parallel with:** Ola Graph · 01, 02, 03, 06
- **Must merge before:** Ola Graph · 05
