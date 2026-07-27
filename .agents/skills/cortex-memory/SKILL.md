---
name: xavier-memory
description: >-
  Legacy MCP-only Xavier memory helpers (create_memory, search_memory, get_memory).
  Prefer the canonical skill `xavier-memory-protocol` for the mandatory
  Fat Search → Page-In → Persist loop over HTTP or MCP. Use this skill only
  when the host requires MCP transport details for those tools.
---

# Xavier Memory MCP Skill (legacy)

> **Status: LEGACY.** Folder name `cortex-memory` is historical — Cortex the product is removed.
> **Canonical protocol:** [`.agents/skills/xavier-memory-protocol/SKILL.md`](../xavier-memory-protocol/SKILL.md)
> Index: [`.agents/skills/README.md`](../README.md)

For the mandatory agent memory loop (MCP + HTTP), always follow **`xavier-memory-protocol`** (`mem_search` → `memory_context`/`get_memory` → `create_memory`). This skill only documents MCP transport quirks.

## Endpoint

- **stdio (Cursor/Claude):** `xavier mcp` via `scripts/mcp/xavier-mcp-cursor.sh`
- **MCP JSON-RPC HTTP:** `http://localhost:8100` (from `xavier http --mcp-port 8100`)
- **Legacy REST MCP bridge:** `http://localhost:8006/mcp` (older clients)

## Preconditions

- Xavier should be running locally (`curl http://localhost:8006/health`).
- `XAVIER_TOKEN` must match between `.env` and the MCP host env.
- Use GitHub Issues for task state and Xavier for durable knowledge.

## Current MCP tools

Canonical names (use these):

- `mem_search`, `memory_context`, `get_memory`, `create_memory`

Also present (aliases / extras):

- `search_memory`, `memory_search`, `memory_save` (deprecated aliases — see canonical skill)
- `list_projects`, `get_project_context`, `sync_gitcore`
- `core_memory_append`, `archival_search` (MemGPT-style helpers)

## Working rules

- **Search before storing:** Prefer `mem_search` (or `archival_search` when using that surface).
- **Page-In on demand:** Use `memory_context` / `get_memory` only for selected ids/paths.
- **Stable references:** Use stable `path` values and meaningful metadata.
- **No ephemera:** Do not store secrets or scratch notes.
- If the tool list differs, inspect `src/server/mcp/` and update this skill instead of guessing.
