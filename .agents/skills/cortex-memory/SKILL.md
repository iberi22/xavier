---
name: xavier-memory
description: Use Xavier cognitive memory as an MCP-backed durable knowledge layer for persistent research, architecture context, and reusable agent memory. Use when the host requires MCP transport and the task needs Xavier tools such as `create_memory`, `search_memory`, `get_memory`, `list_projects`, or `get_project_context`.
---

# Xavier Memory MCP Skill

Use Xavier through MCP when the host tool expects MCP transport. For local scripts and operational automation, prefer the HTTP skill under `skills/xavier-http-curl`.

## Endpoint

Use `http://localhost:8003/mcp` with `streamable-http` transport.

## Preconditions

- Xavier should be running locally.
- `GET /health` should respond before blaming the MCP host.
- Use GitHub Issues for task state and Xavier for durable knowledge.

## Current MCP tools

- `create_memory`
- `search_memory`
- `get_memory`
- `list_projects`
- `get_project_context`
- `sync_gitcore`
- `core_memory_append` (MemGPT style Working Memory mutation)
- `archival_search` (MemGPT style Episodic/Semantic lookup)

## Working rules

- **Search before storing new knowledge:** Always use `search_memory` or `archival_search` to pull past context before assuming something is missing.
- **Dynamic Context Paging:** You act as a cognitive OS. If your pre-digested context is insufficient, explicitly page in memory using `archival_search`, and explicitly page out/append insights using `core_memory_append` to evolve the state in real-time.
- **Stable References:** Use stable `path` values and meaningful metadata.
- **No Ephemera:** Do not store secrets or ephemeral scratch notes. Only store procedural improvements (Harness optimizations) or deep context.
- If the tool list appears different, inspect `src/server/mcp_server.rs` and update this skill instead of guessing.
