# FEATURE: Multi-layer Graph Explorer (Roadmap | Memory KG | Code)

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
A reactive, unified frontend visualization platform mapping project Roadmaps (CRUD), the cognitive Memory Knowledge Graph (Memory KG), and Code symbol structures. The system exposes lightweight endpoints to render high-fidelity force-directed d3 views.

## Architecture & Design
The visualization foundations are unified under the shared types in `panel-ui/src/types/graphLayers.ts` and mapped from API JSON to standard `CanvasGraph` layers via pure adapters in `panel-ui/src/api/graphAdapters.ts`. The UI embeds an accessible sub-tablist switcher ('Roadmap / Memory KG / Code') inside the main settings pane.

## Implementation Paths
- `src/api/graph.rs` (Roadmap graph storage handlers and SQLite backends)
- `src/cli/handlers/code.rs` (code graph overview and ego routers)
- `panel-ui/src/components/GraphCanvas.tsx` (the canvas layer and rendering triggers)
- `panel-ui/src/api/graphAdapters.ts` (JSON-to-Canvas adapters)

## Sub-features
- **Roadmap CRUD APIs:** GET/POST endpoints backing custom user roadmap visualizations.
- **Memory Graph Adapters:** Exposes EntityGraph entities and relations.
- **Code Graph Visualizer:** Outputs Force-graph payloads of symbols and call trees.
- **Unified Graph Canvas:** Tabbed React/Tauri component switching layers fluidly.
- **EntityGraph Durability:** Serializes the active conceptual graph into the database.

## Test References
- `server::panel::tests::graphs_crud` verifying roadmap storage.
- `storage::migrations::tests::legacy_entities_missing_workspace_id_survives_v1_closure` checking index migrations.
- `panel-ui graphAdapters + roadmapGraph tests` running on Vitest.

## Known Issues & Notes
- Live smoke verified on Windows systems. Restores legacy entity naming columns and missing workspace boundaries successfully.
