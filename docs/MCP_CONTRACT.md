# Xavier 1.0 MCP Contract

**Last Updated:** 2026-07-27
**Protocol Version:** 2025-03-26
**Server Name:** `xavier-memory`

## Overview

Xavier exposes a unified MCP (Model Context Protocol) interface. Prefer the **JSON-RPC MCP** transport — do not confuse it with the legacy REST listing under the main HTTP API.

| Transport | How to start | Endpoint / port |
|-----------|--------------|-----------------|
| **JSON-RPC MCP (canonical)** | `xavier mcp` or `xavier http --mcp-port 8100` | HTTP+SSE on **:8100** (`POST /mcp`, `GET /mcp`) |
| **STDIO MCP** | `xavier mcp` (stdio mode for clients) | Line-delimited JSON-RPC |
| **Legacy REST (deprecated)** | Main HTTP API | `GET /mcp/tools` on **:8006** — `deprecated: true`; prefer JSON-RPC on :8100 |

Both JSON-RPC transports use the same `dispatch_mcp_value` dispatcher and expose the identical tool set (~27 tools including aliases for compatibility).

### Transport Details

| Feature | JSON-RPC MCP (:8100) | STDIO |
|---------|----------------------|-------|
| Endpoint | `POST http://localhost:8100/mcp` | `xavier mcp` |
| JSON-RPC Batch | ✅ Yes | Line-delimited JSON |
| Session Header | `mcp-session-id` | N/A |
| Auth | `X-Xavier-Token` header | Inherits from env |

## Canonical agent loop

Progressive disclosure (required for agents):

1. **`mem_search`** — Fat index: structured candidates `{id,path,score,snippet,kind}` (no full body by default)
2. **`memory_context`** / **`get_memory`** — Page-in full or bounded content by ids (or query)
3. **`create_memory`** — Persist new durable knowledge

Aliases (compat, same handlers):

| Alias | Prefer instead |
|-------|----------------|
| `search_memory` | `mem_search` |
| `memory_search` | `mem_search` (now same structured fat-index path) |
| `mem_context` | `memory_context` |

## MCP Protocol Handshake

### initialize

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": "2025-03-26",
    "capabilities": {},
    "clientInfo": { "name": "...", "version": "..." }
  }
}
```

**Response** includes `serverInfo.name: "xavier-memory"` and `capabilities.tools: {}`.

### notifications/initialized

When the client has completed initialization, send as a notification (no `id` field).

## Tools — Xavier 1.0 Contract

### Canonical Memory Tools

| Tool | Description | Required Params |
|------|-------------|-----------------|
| `create_memory` | Create a new memory document | `path`, `content` |
| `mem_search` | Fat index search (progressive disclosure step 1) | `query` |
| `memory_context` | Page-in by ids or query (step 2) | `query` **or** `ids` |
| `get_memory` | Get a specific memory by ID | `id` |
| `stats` | Get Xavier memory statistics | _(none)_ |

### Deprecated / alias search tools

| Tool | Notes |
|------|-------|
| `search_memory` | Deprecated — use `mem_search` |
| `memory_search` | Deprecated — same structured handler as `mem_search` |
| `mem_context` | Alias of `memory_context` |

### Project Tools

| Tool | Description | Required Params |
|------|-------------|-----------------|
| `list_projects` | List all projects | _(none)_ |
| `get_project_context` | Get full context for a project | `project_id` |

### Utility Tools

| Tool | Description | Required Params |
|------|-------------|-----------------|
| `sync_gitcore` | Sync docs from a GitCore project | `project_path` |
| `health_check` | Structured health + `toolsCount` (core + memory + context) | _(none)_ |

### Gestalt MemoryFragment Tools

These tools provide compatibility with the Gestalt MCP protocol. Each has a canonical short name and a `memoryfragment_*` alias.

| Tool | Alias | Description | Required Params |
|------|-------|-------------|-----------------|
| `save_fragment` | `memoryfragment_save` | Save a memory fragment | `agent_id`, `content`, `context` |
| `search_fragments` | `memoryfragment_search` | Search memory fragments | `query` |
| `get_recent_fragments` | `memoryfragment_recent` | Get recent fragments for agent | `agent_id` |
| `memoryfragment_get` | — | Get a specific fragment by ID | `id` |
| `memoryfragment_delete` | — | Delete a fragment by ID | `id` |

## Security Scanning

All MCP tool inputs (string arguments) are scanned for prompt injection and security threats before execution. Two scanning layers are applied:

1. **Generic pre-scan** (all tools): Every string argument is checked by `SecurityService.scan()`. Arguments named `id` are exempt (safe identifiers).
2. **Dedicated content scan** (MemoryFragment tools only): The `content` and `query` fields get a second scan via `secure_mcp_external_input()` which returns rich blocked responses with detection details.

If a security violation is detected, the tool returns an MCP error `-32000` with a description of the violation.

## Example: create_memory

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "create_memory",
    "arguments": {
      "path": "projects/my-project/notes",
      "content": "Important observation about the architecture",
      "kind": "semantic",
      "evidence_kind": "observation",
      "namespace": {
        "project": "my-project",
        "agent_id": "agent-1"
      },
      "provenance": {
        "source_app": "my-app",
        "source_type": "chat"
      }
    }
  }
}
```

## Example: mem_search

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "mem_search",
    "arguments": {
      "query": "architecture patterns",
      "limit": 5,
      "filters": {
        "project": "my-project"
      }
    }
  }
}
```

## Example: save_fragment

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "save_fragment",
    "arguments": {
      "agent_id": "agent-1",
      "content": "Observed that the authentication module uses JWT tokens",
      "context": "observation",
      "tags": ["auth", "security"],
      "importance": 0.8
    }
  }
}
```

## Validation Rules

MemoryFragment inputs enforce strict validation:

| Field | Max Length | Allowed Characters |
|-------|-----------|-------------------|
| `agent_id` | 128 chars | ASCII alphanumeric, `.`, `_`, `-` |
| `context` | 128 chars | ASCII alphanumeric, `.`, `_`, `-` |
| Tags (each) | 64 chars | ASCII alphanumeric, `.`, `_`, `-` |
| Tags (count) | 32 tags | — |
| `repo_url` / `file_path` / `chunk_id` | 2048 chars | No control characters |
| `importance` | — | Float 0.0–1.0 (default 0.5) |
| `limit` | — | Integer 1–100 (default 10) |

## Error Handling and Codes

Xavier uses standard JSON-RPC 2.0 error codes and custom Xavier-specific codes for MCP tool calls.

| Code | Name | Description |
|------|------|-------------|
| `-32000` | `XAVIER_ERROR_SECURITY` | Security policy violation (e.g., prompt injection) |
| `-32001` | `XAVIER_ERROR_VALIDATION` | Missing parameters or invalid argument format |
| `-32002` | `XAVIER_ERROR_NOT_FOUND` | Requested resource (memory, project) not found |
| `-32601` | `Method not found` | Standard JSON-RPC error for unknown methods |
| `-32603` | `Internal error` | Unhandled internal exception |
