# Xavier 1.0 MCP Contract

**Last Updated:** 2026-09-27
**Protocol Version:** 2025-03-26
**Server Name:** `xavier-memory`

## Overview

Xavier exposes a unified MCP (Model Context Protocol) interface. Prefer the **JSON-RPC MCP** transport — do not confuse it with the legacy REST listing under the main HTTP API.

| Transport | How to start | Endpoint / port |
|-----------|--------------|-----------------|
| **JSON-RPC MCP (canonical)** | `xavier mcp` or `xavier serve` / `xavier http --mcp-port 8100` | HTTP+SSE on **:8100** (`POST /mcp`, `GET /mcp`) |
| **STDIO MCP** | `xavier mcp` (stdio mode for clients) | Line-delimited JSON-RPC |
| **Legacy REST (deprecated)** | Main HTTP API | `GET /mcp/tools` on **:8006** — `deprecated: true`; prefer JSON-RPC on :8100 |

Both JSON-RPC transports use the same `dispatch_mcp_value` dispatcher and expose the identical tool set: **42 tools** — 16 core (`get_xavier_core_tools`), 18 memory (`get_xavier_memory_tools`) and 8 context (`get_xavier_context_tools`), including aliases for compatibility. `health_check` reports this same total as `toolsCount`.

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
| `xavier_local_status` | Report local-first operation mode and reachability | _(none)_ |

`health_check` returns a `MCPHealthResult` with `status`, `toolsCount`, `handshakeOk`, `memoryStoreOk`, `embeddingOk` and `mcpProtocol`. The two verdict fields are computed as follows:

- **`memoryStoreOk`** — the memory store answers a query (`memory.count()` succeeds) **and** no measured `sqlite_integrity` check failed. An integrity check that was not run does not flip the flag: only a measured failure does.
- **`embeddingOk`** — the embedding provider is `connected`, or the fallback embedder succeeded (`fallback_success`).

`isError` is reserved for tool *execution* failure, not host state. `health_check` sets it only when the collected health status is `unhealthy` or `critical`; `warn`/`degraded` alerts ride inside the payload with `isError: false`.

`xavier_local_status` takes no input and returns `mode` (`local-healthy` · `local-degraded` · `cloud-fallback` · `disabled`), `provider_setting` (from `XAVIER_PROVIDER`, else `XAVIER_MODEL_PROVIDER`, else `local`), `llm_reachable`, `embedding_reachable`, `ollama_reachable` and an empty `fallback_chain`. `embedding_reachable` is false only when the embedding health level is `Unhealthy`; `ollama_reachable` requires an LLM that is reachable **and** a `local`/`ollama` provider. Call it before delegating reasoning to Xavier. `isError` is always `false`.

### Node Self-Management Tools

Read-only guardian of the host plus two escalation paths. All of them take an empty or near-empty `inputSchema` and return a `structuredContent` object (the same JSON is also mirrored into the text content block).

| Tool | Description | Required Params |
|------|-------------|-----------------|
| `sys_health` | Read-only host snapshot (PSI, swap, load, top processes, D-state, threshold alerts) | _(none)_ |
| `log_scan` | Scan logs under `~/.xavier/logs` (or fallback) with cursor, redaction and Telegram Polling Dead detection | _(none)_ |
| `env_status` | systemd service states, TCP connectivity probes, PSI and swap snapshot | _(none)_ |
| `ticket_create` | Create a GitHub issue or Maloca backlog entry with fingerprint dedupe and rate limiting | `title`, `body`, `severity` |

#### `sys_health`

Purpose: a read-only snapshot of the machine Xavier runs on, with no side effects. Registered description (verbatim): *"Snapshot del HOST (guardian del nodo): PSI (cpu/memory/io avg10/60/300), swap usado, load average, top 10 procesos por RSS, conteo D-state y alertas con umbrales (psi.io.full.avg10>50% critical, swap>80% critical, VmSwap>4GB warn). Read-only, sin efectos"*.

Input: none (`properties: {}`). Any argument is ignored.

Output keys:

| Key | Content |
|-----|---------|
| `overall` | The host snapshot verdict: `healthy`, `degraded` or `critical` (`SystemSnapshot.overall`). |
| `components` | The full in-process `HealthResponse` — `status`, `version`, `uptime_secs`, `system`, `database`, `embedding`, `mesh`, `telegram`, `auth`, `dependency_graph`, `checks`, `embedding_coverage`, `degraded_reasons`. |
| `active_gaps` | Gaps from `analyze_gaps` over a `BenchmarkSnapshot` synthesized from the live health response. |
| `db_integrity` | Result of the `sqlite_integrity` check. `null` when the SQLite integrity check was **not measured** on this path, otherwise a boolean. It is never a fabricated `false`. |
| `last_experiment` | First experiment of the newest non-truncated entry in `.xavier/improvement-history.json`, or `null`. Truncated improvement cycles are skipped, because such an entry accepted nothing. |
| `system_snapshot` | `SystemSnapshot`: `psi` (per-resource `some`/`full` `avg10`/`avg60`/`avg300`), `swap`, `load_avg`, `top_rss`, `d_state_count`, `alerts`, `overall`. |

Measured-vs-unmeasured discipline: the synthesized `BenchmarkSnapshot` fills `timestamp_secs`, `memory_hit_rate` (a database size proxy), `mesh_peers_reachable`, `health_status`, `db_integrity_ok` and `total_documents` from live data. `recall_at_k`, `precision`, `avg_latency_ms`, `p99_latency_ms` and `cache_hit_rate` are not measured on this path and use the `-1.0` sentinel; `test_iterations` is `0`. Because `analyze_gaps` only opens a gap for a metric `> 0.0`, a `-1.0` sentinel never produces a gap — an unmeasured benchmark cannot masquerade as degraded performance. For the same reason an unmeasured integrity check is treated as OK for gap analysis, so it never raises a `Critical` `db_integrity` gap.

`isError` is `true` only when `overall == "critical"`, i.e. at least one alert has critical severity.

#### `log_scan`

Purpose: read the host's Xavier logs, redact secrets, filter, and detect a dead Telegram poller.

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `since` | string | no | — | Filter logs since an RFC3339 timestamp. |
| `level_min` | string | no | — | Minimum log level to show. |
| `pattern` | string | no | — | Regex pattern to match. |
| `source` | string | no | — | `xavier` · `hermes` · `journalctl`. Accepted and carried into the scan args, but this implementation always reads the resolved log directory (see below). |
| `max_entries` | number | no | `500` | Entry cap. |

Output (`LogScanResult`): `entries` (each with `timestamp`, `level`, `message`, `source`, `count`, optional `first_seen`/`last_seen`), `truncated`, `cursor` (`last_file`, `last_line`), `histogram` (level → count), `telegram_polling_dead`, `earliest_timestamp`, `latest_timestamp`, `cursor_reset`, `cursor_reset_reason`. Lines are always read from `~/.xavier/logs` when that directory exists, otherwise from the temp-dir fallback — `source` does not currently switch the directory. Every line passes through regex secret redaction before parsing, so `bearer …`, `api_key=…`, `token=…` and `password=…` values are replaced with `[REDACTED]`.

Supplying `since` starts from a fresh cursor instead of the persisted one. Independently, the persisted cursor is dropped and reported when its file rotated away or is stale (more than one rotation behind the newest log); in that case only the newest log is scanned, so history is never replayed as if it were the current state. Both situations are explained by `cursor_reset` and `cursor_reset_reason`.

`isError` is `true` when `telegram_polling_dead` is set, which happens when at least two matched entries mention Telegram or polling **and** a failure word (`failed`, `error`, `close-wait`, `dead`, `retry`), or when any single entry mentions `close-wait`/`close_wait`.

#### `env_status`

Purpose: report the node's dependency environment rather than the host alone — service units, outbound TCP reachability, and the same PSI/swap snapshot.

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `include_processes` | boolean | no | — | `false` clears the process list. |
| `top_n` | number | no | — | Limit the process list; capped at 20. |

Output (`EnvStatusResult`): `psi`, `swap`, `load_avg`, `top_processes`, `services` (status per allowlisted unit — `xavier.service`, `hermes.service`, `peerjs.service`, `openclaw.service` — with the resolved systemd scope, e.g. `active (user)`, `not-found`), `connectivity` (`dns_google` probed at `8.8.8.8:53`, `telegram_api` at `api.telegram.org:443`; each 2s timeout, value `established` or `failed: …`), `alerts`, `overall`. A unit that does not exist is reported as `not-found`, never as a bare `inactive`.

`isError` is `true` only when `overall == "critical"`.

#### `ticket_create`

Purpose: let an agent escalate a finding without spamming a tracker.

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `title` | string | yes | — | Title (max 120 chars). |
| `body` | string | yes | — | Body/evidence (max 8KB). |
| `labels` | string[] | no | — | Optional label list. |
| `severity` | string | yes | `warn` | `critical` · `warn`. |
| `fingerprint` | string | no | derived | Override for the dedupe key. |
| `backend` | string | no | `maloca` | `github` · `maloca`. |

Output (`TicketCreateResult`): `id`, `url`, `deduplicated`, `backend`. Without an explicit `fingerprint`, one is computed as the SHA-256 of trimmed `title` + `severity`. A fingerprint already present in `~/.xavier/state/tickets.json` returns the recorded `id` with `deduplicated: true` and an empty `url`. `github` shells out to `gh issue create` and falls back to the Maloca store when `gh` is missing or fails.

A `GITHUB_TOKEN` lease is taken for the duration of the call via the `KeyLendingEngine` and revoked afterwards.

`isError` is `false` on every successful call, including deduplicated hits. The anti-loop rate limit is a hard error instead: more than 3 created tickets within the trailing hour aborts the tool call.

### Code Graph Tools

| Tool | Description | Required Params |
|------|-------------|-----------------|
| `get_code_graph` | Return the portable code graph dump (`.xavier/codegraph.json`) | _(none)_ |
| `codegraph_explore` | Search the code graph for symbols matching a query | `query` |
| `codegraph_route` | Route a query through the graph with a confidence margin verdict | `query` |
| `codegraph_gods` | Top structurally central, connected symbols (gods/hubs) | _(none)_ |
| `trace_path` | Trace the dependency path or call chain from a symbol | `symbol` |

#### `get_code_graph`

Input: none. Reads the dump path from server state, falling back to `codegraph_dump_path_for(cwd)`.

Output: when the dump file exists, the raw dump JSON as the text content (with no `structuredContent`); `isError: false`. When it is missing or stale, a live fallback summary is returned instead of a false "not found": `source: "live_db"`, `dump_path`, `dump_present: false`, a `hint` to run `xavier code dump .` or `xavier code scan .`, `stats` (`total_files`, `total_symbols`, `total_imports`) and up to 10 `hubs` (`name`, `file`, `incoming`, `outgoing`). An unreadable or malformed dump file surfaces as a tool error.

#### `codegraph_explore`

| Param | Type | Required | Default | Range |
|-------|------|----------|---------|-------|
| `query` | string | yes | — | Part of a symbol name. |
| `limit` | number | no | `20` | Clamped to 1–100. |

Output: `returned` (number of symbols in `symbols`) and `symbols` — each `Symbol` carries `id`, `stable_id`, `name`, `kind`, `lang`, `file_path`, `start_line`/`end_line`, `start_col`/`end_col`, `signature`, `parent`, `complexity`. `isError` is always `false`; a missing `query` is a validation error.

#### `codegraph_route`

| Param | Type | Required | Default | Range |
|-------|------|----------|---------|-------|
| `query` | string | yes | — | Query or symbol name. |
| `limit` | number | no | `10` | Clamped to 1–100. |

Combines exact-name matches with centrality-ranked candidates. Output: `mode` (`ExactName` when an exact hit answered the query, otherwise `GraphRanked`), `margin` (margin percentage), `confident`, `refusal` (the inverse of `confident` — read it as "this is a starting point, not a verdict"), `est_tokens`, `total`, `shown` and `hits` (`symbol`, `score`). `isError` is always `false`.

#### `codegraph_gods`

| Param | Type | Required | Default | Range |
|-------|------|----------|---------|-------|
| `limit` | number | no | `10` | Clamped to 1–100. |

Over-fetches `limit * 10` candidates (clamped to 50–200), then filters them: generic constructors — `new`, `default`, `from`, `with_capacity`, `clone`, compared case-insensitively — are excluded, and results are **deduplicated per symbol** (by `stable_id`, falling back to `file_path:start_line:name`), so identically named hubs in different modules stay distinct. Output: `returned` and `god_nodes` (each with its `symbol`, `incoming`, `outgoing`, `total`). When the filter leaves nothing, the result is `returned: 0` with an empty `god_nodes` array and a `reason` explaining that generic constructors were excluded. `isError` is always `false`.

#### `trace_path`

| Param | Type | Required | Default | Range |
|-------|------|----------|---------|-------|
| `symbol` | string | yes | — | Stable ID or symbol name. |
| `max_depth` | number | no | `3` | Clamped to 1–8. |
| `reverse` | boolean | no | `false` | `true` traces callers / reverse dependencies. |
| `edge_type` | string | no | — | Filter, e.g. `Calls`, `References`, `Imports`. |
| `limit` | number | no | `100` | Clamped to 1–1000. |

Output: `symbol`, `direction` (`callers` when `reverse` is true, `dependencies` otherwise) and `edges` — each `CodeEdge` carries `from_symbol`, `to_symbol`, `edge_type`, `file_path`, `line`, `confidence` and optional `metadata`. An unrecognized `edge_type` string is ignored rather than rejected. `isError` is always `false`; a missing `symbol` is a validation error.

### Espacio Channel Tools

| Tool | Description | Required Params |
|------|-------------|-----------------|
| `espacio_channel_list` | List channels/messages for a specified space | `space_id` |
| `espacio_channel_create` | Create a channel within a space | `space_id`, `name` |

`espacio_channel_list` takes `space_id` (string) and returns `space_id`, `messages` and `count`. Each `ChannelMessage` carries `seq` (monotonic within the space), `space_id`, `author`, `content` and `created_at`.

`espacio_channel_create` takes `space_id` and `name` (both strings) and returns `space_id`, `channel_name`, a `status` of `created`, and the posted `message` — posted under the author `mcp_operator`, with `content` clipped to 4KB.

Both set `isError: false` on success; a missing required argument is a validation error.

### Gestalt MemoryFragment Tools

These tools provide compatibility with the Gestalt MCP protocol. Each has a canonical short name and a `memoryfragment_*` alias.

| Tool | Alias | Description | Required Params |
|------|-------|-------------|-----------------|
| `save_fragment` | `memoryfragment_save` | Save a memory fragment | `agent_id`, `content`, `context` |
| `search_fragments` | `memoryfragment_search` | Search memory fragments | `query` |
| `get_recent_fragments` | `memoryfragment_recent` | Get recent fragments for agent | `agent_id` |
| `memoryfragment_get` | — | Get a specific fragment by ID | `id` |
| `memoryfragment_delete` | — | Delete a fragment by ID | `id` |

### Gestalt Event Bus Integration Contract

Gestalt agents connect to Xavier via the `GestaltAdapter` inbound plugin (`src/adapters/inbound/gestalt/mod.rs`). The event bus bridge subscribes to `XavierEventBus` and coordinates agent lifecycle events (`AgentTaskStarted`, `AgentTaskCompleted`, `AgentTaskFailed`) with the Gestalt runtime ecosystem.

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

## Migration Guide: Deprecated `/mcp/tools` REST Endpoint -> JSON-RPC MCP (:8100)

The REST endpoint `GET /mcp/tools` on the main API port (`:8006`) is **deprecated**.

### Deprecation Response Signals

When calling `GET http://localhost:8006/mcp/tools`, Xavier emits standard deprecation headers and payload:

- **HTTP Headers:**
  - `deprecation: true`
  - `link: </mcp>; rel="successor-version"; title="JSON-RPC MCP on port 8100"`
- **Response Body:**
  ```json
  {
    "deprecated": true,
    "note": "Legacy REST /mcp/tools on the main HTTP API (:8006). Prefer JSON-RPC MCP on port 8100 via `xavier mcp` or `xavier http --mcp-port`. Canonical agent loop: mem_search → memory_context/get_memory → create_memory.",
    "tools": [ ... ]
  }
  ```

### Migrating to Canonical JSON-RPC MCP (:8100)

To migrate from the legacy REST endpoint to the full JSON-RPC MCP implementation:

1. **Connect to the MCP HTTP port** (default: `http://localhost:8100/mcp`, enabled via `xavier http --mcp-port 8100` or `xavier mcp`).
2. **Set authentication and protocol headers**:
   - `X-Xavier-Token: <XAVIER_TOKEN>`
   - `Content-Type: application/json`
   - `MCP-Protocol-Version: 2026-07-28`
3. **List available tools via `tools/list`**:
   ```bash
   curl -X POST http://localhost:8100/mcp \
     -H "Content-Type: application/json" \
     -H "X-Xavier-Token: $XAVIER_TOKEN" \
     -d '{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}'
   ```
4. **Execute tool calls via `tools/call`**:
   ```bash
   curl -X POST http://localhost:8100/mcp \
     -H "Content-Type: application/json" \
     -H "X-Xavier-Token: $XAVIER_TOKEN" \
     -d '{
       "jsonrpc": "2.0",
       "id": 2,
       "method": "tools/call",
       "params": {
         "name": "mem_search",
         "arguments": { "query": "memory architecture" }
       }
     }'
   ```
