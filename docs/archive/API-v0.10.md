# Xavier REST API

Version: `0.10.0-12-06-2026`

Base URL: `http://localhost:8006`

## Authentication

Most endpoints are protected by token middleware and require:

```http
X-Xavier-Token: <token>
Content-Type: application/json
```

Public endpoints: `/health`, `/ready`, `/readiness`, `/build`, `/v1/version`, `/panel`, and `/panel/assets/{path}`.

Set the token before starting Xavier:

```bash
export XAVIER_TOKEN="change-me"
xavier http 8006
```

## Health and Build

| Method | Path | Description |
|---|---|---|
| `GET` | `/health` | Runtime health, version, provider status, and storage indicators. |
| `GET` | `/ready` | Readiness alias. |
| `GET` | `/readiness` | Readiness check. |
| `GET` | `/build` | Build metadata. |
| `GET` | `/v1/version` | Version metadata. |
| `GET` | `/system/alerts` | Current system alerts. Requires token. |

```bash
curl http://localhost:8006/health
```

## V1 Memories

Canonical v1 CRUD endpoints from `src/server/v1_api.rs`.

### List Memories

`GET /v1/memories?limit=100&offset=0`

Response:

```json
{
  "memories": [
    {
      "id": "01J...",
      "memory": "content",
      "user_id": "default",
      "metadata": {}
    }
  ],
  "pagination": {
    "total": 1,
    "limit": 100,
    "offset": 0
  }
}
```

### Add Memory

`POST /v1/memories`

Request:

```json
{
  "text": "Decision: use signed mesh manifests",
  "user_id": "belal",
  "metadata": {"project": "xavier"},
  "kind": "decision",
  "evidence_kind": "direct",
  "namespace": {"project": "xavier", "session_id": "session-001"},
  "provenance": {"source_app": "codex", "source_type": "manual"}
}
```

Alternative chat-style request:

```json
{
  "messages": [
    {"role": "user", "content": "What changed?"},
    {"role": "assistant", "content": "Mesh Phase 2 shipped."}
  ],
  "user_id": "belal"
}
```

Response:

```json
{"status": "ok", "message": "Memory added successfully", "id": "01J..."}
```

Example:

```bash
curl -X POST "$BASE/v1/memories" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"text":"Decision: use signed mesh manifests","user_id":"belal"}'
```

### Get Memory

`GET /v1/memories/{id}`

Response:

```json
{
  "status": "ok",
  "memory": {
    "id": "01J...",
    "memory": "content",
    "user_id": "belal",
    "metadata": {}
  }
}
```

### Search Memories

`POST /v1/memories/search`

Request:

```json
{
  "query": "mesh manifests",
  "limit": 5,
  "filters": {
    "kinds": ["decision"],
    "project": "xavier",
    "user_id": "belal",
    "source_app": "codex"
  },
  "active_zones": ["working", "semantic"]
}
```

Response:

```json
{
  "status": "ok",
  "results": [
    {"id": "01J...", "memory": "content", "user_id": "belal", "metadata": {}}
  ]
}
```

### Update Memory

`PUT /v1/memories/{id}`

Request accepts the same shape as add. Omitted fields preserve existing content/path where supported.

```bash
curl -X PUT "$BASE/v1/memories/$ID" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"text":"Updated memory","metadata":{"revision_note":"docs"}}'
```

### Delete Memory

`DELETE /v1/memories/{id}`

Response:

```json
{"status": "ok", "message": "Memory deleted successfully"}
```

### Export Memories

`GET /v1/memories/export?public=true`

Exports memory documents. `public=true` excludes private records.

## Legacy Memory HTTP Endpoints

These are used by the CLI server and remain supported.

| Method | Path | Description |
|---|---|---|
| `POST` | `/memory/add` | Add a memory with `content`, optional `path`, `metadata`, `title`, `cluster_id`, `level`, and `relation`. |
| `POST` | `/memory/search` | Search memories with `query`, `limit`, optional `filters`, and `active_zones`. |
| `POST` | `/memory/update` | Update by `id`, with `content`, optional `path`, and `metadata`. |
| `POST` | `/memory/delete` | Delete by `id` or `path`. |
| `GET` | `/memory/stats` | Basic workspace and version stats. |
| `GET` | `/memory/export?public=true&limit=1000` | Export memory records. |
| `POST` | `/memory/export-pack` | Export a context pack (`.xcp`) for a topic. |
| `POST` | `/memory/query` | Query memory through the headless-compatible query shape. |
| `POST` | `/memory/decay` | Run memory decay. |
| `POST` | `/memory/consolidate` | Run memory consolidation. |
| `DELETE` | `/memory/evict` | Evict by priority or low quality threshold. |
| `POST` | `/memory/manage` | Run decay, consolidate, and low-quality eviction. |
| `POST` | `/memory/timeline/query` | Query timeline slices. |

Add example:

```bash
curl -X POST "$BASE/memory/add" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"content":"Remember this","path":"notes/remember-this","metadata":{"kind":"note"}}'
```

Export pack request:

```json
{"topic": "mesh sync", "max_level": 3}
```

## Memory Retrieve, Curate, Decay, Consolidate, Reflect

The HTTP API module also defines these workspace-level operations:

| Method | Path | Description |
|---|---|---|
| `POST` | `/memory/retrieve` | Retrieve layered memory for a query. |
| `POST` | `/memory/export/pack` | Export a context pack through the HTTP API module. |
| `POST` | `/memory/curate` | Apply a curation action to memory. |
| `POST` | `/memory/manage` | Auto-manage memory. |
| `POST` | `/memory/decay` | Apply decay policy. |
| `POST` | `/memory/consolidate` | Consolidate memory. |
| `POST` | `/memory/reflect` | Run reflection task. |

If your running binary exposes `/memory/export-pack` instead of `/memory/export/pack`, use the CLI-server path shown above.

## Mesh API

Mesh endpoints are token-protected and implemented by `src/server/v1_api.rs`.

| Method | Path | Description |
|---|---|---|
| `GET` | `/v1/mesh/identity` | Return local node identity public info. |
| `POST` | `/v1/mesh/handshake` | Verify a remote node signature and optional pairing secret. |
| `GET` | `/v1/mesh/manifest` | Return ACL-filtered sync manifest. |
| `POST` | `/v1/mesh/chunks/request` | Request wanted chunk payloads. |
| `POST` | `/v1/mesh/chunks/push` | Push chunk payloads to the local node. |
| `GET` | `/v1/mesh/cloud` | Read configured cloud node settings. |
| `PUT` | `/v1/mesh/cloud` | Update cloud node settings. |
| `GET` | `/v1/mesh/data_commons/opt_in` | Read Data Commons opt-in settings. |
| `POST` | `/v1/mesh/data_commons/opt_in` | Update Data Commons consent state. |
| `POST` | `/v1/mesh/session/{session_id}/share` | Share a session bundle with a trusted peer. |
| `GET` | `/v1/mesh/peers` | List peers. |
| `POST` | `/v1/mesh/peers/pair` | Pair a peer. |
| `POST` | `/v1/mesh/peers/decode` | Decode a pairing code. |
| `POST` | `/v1/mesh/peers/generate-code` | Generate a pairing code. |
| `PUT` | `/v1/mesh/peers/{node_id}/acl` | Update peer ACL. |
| `DELETE` | `/v1/mesh/peers/{node_id}` | Remove a peer. |

Handshake request:

```json
{
  "node_id": "node_abc",
  "public_key_hex": "abcd...",
  "nonce": "random-nonce",
  "signature_hex": "abcd...",
  "pairing_secret": "optional-secret"
}
```

Manifest request:

```bash
curl "$BASE/v1/mesh/manifest?node_id=node_abc&timestamp=1710000000&nonce=n1&signature=abcd" \
  -H "X-Xavier-Token: $XAVIER_TOKEN"
```

Chunk request:

```json
{
  "requesting_node_id": "node_abc",
  "wanted_hashes": ["sha256..."],
  "timestamp": 1710000000,
  "nonce": "n1",
  "signature_hex": "abcd..."
}
```

Cloud update:

```json
{"url": "https://cloud.example", "token": "cloud-token", "instance_id": "xavier-prod"}
```

Data Commons opt-in:

```json
{"enabled": true, "consent_given": true, "wallet_address": "optional-wallet"}
```

## Session API

| Method | Path | Description |
|---|---|---|
| `GET` | `/v1/sessions/{session_id}/export` | Export a session bundle. |
| `POST` | `/v1/sessions/import` | Import a session bundle. |
| `POST` | `/session/compact` | Compact a session. |
| `POST` | `/xavier/events/session` | Record a session event. |

Export:

```bash
curl "$BASE/v1/sessions/session-001/export" \
  -H "X-Xavier-Token: $XAVIER_TOKEN"
```

Import:

```bash
curl -X POST "$BASE/v1/sessions/import" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  --data @session-bundle.json
```

## Code Graph API

| Method | Path | Description |
|---|---|---|
| `POST` | `/code/scan` | Scan and index a codebase path. |
| `POST` | `/code/index` | Index code. |
| `POST` | `/code/find` | Find symbols. |
| `POST` | `/code/context` | Retrieve code context. |
| `GET` | `/code/stats` | Code graph stats. |
| `POST` | `/code/dependencies` | Outgoing dependencies. |
| `POST` | `/code/reverse-dependencies` | Incoming dependencies. |
| `POST` | `/code/call-chain` | Trace call chains. |
| `GET` | `/code/hubs` | Highly connected symbols. |
| `GET` | `/code/hotspots` | Complexity hotspots. |

Find request:

```json
{"query": "MemoryManager", "limit": 10, "kind": "struct"}
```

## Provider, Usage, Billing, Tasks

| Method | Path | Description |
|---|---|---|
| `GET` | `/v1/account/usage` | Account usage and billing summary. |
| `GET` | `/v1/usage/status/{provider}` | Provider quota status. |
| `POST` | `/v1/usage/update` | Manually set provider usage percentage. |
| `POST` | `/v1/usage/cooldown` | Set provider cooldown minutes. |
| `POST` | `/v1/usage/track` | Track usage. |
| `GET` | `/v1/usage/summary/{provider}` | Provider usage summary. |
| `GET` | `/v1/providers/quota` | Provider quotas. |
| `GET` | `/v1/provider/status` | Provider status. |
| `GET` | `/v1/provider/list` | Available providers. |
| `POST` | `/v1/provider/set` | Switch provider. |
| `POST` | `/v1/provider/auto` | Set auto-routing strategy. |
| `GET` | `/v1/tasks` | List tasks with optional `project`, `status`, and `search`. |
| `POST` | `/v1/tasks/sync` | Sync tasks. |

## Secrets and Security

| Method | Path | Description |
|---|---|---|
| `POST` | `/secrets/lend` | Lend a secret to an agent for a TTL. |
| `GET` | `/secrets/leases` | List active leases. |
| `POST` | `/secrets/revoke` | Revoke a lease token. |
| `GET` | `/secrets/status/{token}` | Check lease status. |
| `POST` | `/security/scan` | Scan input/security state. |
| `GET` | `/security/tokens` | List API tokens. |
| `POST` | `/security/tokens` | Create API token. |
| `DELETE` | `/security/tokens/{id}` | Revoke API token. |
| `POST` | `/security/tokens/{id}/rotate` | Rotate API token. |
| `POST` | `/v1/security/approve` | Approve a security action. |
| `POST` | `/xavier/verify/save` | Verify and save payload. |

Lend request:

```json
{"secret_name": "OPENAI_API_KEY", "agent_id": "agent-1", "ttl_seconds": 3600}
```

## MCP and Skills

| Method | Path | Description |
|---|---|---|
| `GET` | `/mcp/tools` | List MCP-compatible HTTP tools. |
| `GET` | `/api/skill/list` | List skills. |
| `GET` | `/skills` | List skills alias. |
| `POST` | `/api/skill/dispatch` | Dispatch a skill. |
| `GET` | `/api/memory/health` | Memory health for skill system. |

The stdio MCP server is started with:

```bash
xavier mcp
```

## Headless API

| Method | Path | Description |
|---|---|---|
| `GET` | `/v1/system/health` | Headless health. |
| `GET` | `/v1/system/scan` | Headless system scan. |
| `GET` | `/v1/system/info` | System info. |
| `POST` | `/v1/chat/completions` | Headless chat completions. |
| `GET` | `/v1/providers` | Provider list. |
| `GET` | `/v1/providers/status` | Provider status. |
| `POST` | `/v1/providers/switch` | Switch provider. |
| `GET` | `/v1/quota` | Quota summary. |
| `GET` | `/v1/usage` | Usage summary. |
| `GET` | `/v1/agents` | Agent list. |
| `POST` | `/v1/agents/spawn` | Spawn agent. |
| `POST` | `/v1/memory/search` | Headless memory search. |
| `POST` | `/v1/memory/add` | Headless memory add. |
| `GET` | `/v1/memory/export` | Headless memory export. |

Headless E2E aliases:

| Method | Path |
|---|---|
| `GET` | `/headless/health` |
| `GET` | `/headless/context` |
| `POST` | `/headless/memory/search` |
| `GET` | `/headless/tools` |
| `POST` | `/headless/tools/{name}` |
| `GET` | `/headless/provider/status` |

## Navigation API

| Method | Path | Description |
|---|---|---|
| `GET` | `/v1/nav/ls?path=/` | List memory path. |
| `POST` | `/v1/nav/cd` | Validate/change path target. |
| `GET` | `/v1/nav/pwd` | Current path. |
| `GET` | `/v1/nav/affected?path=...&depth=2` | Impact analysis. |
| `GET` | `/v1/nav/visualize` | Memory graph visualization. |

## Panel API

| Method | Path | Description |
|---|---|---|
| `GET` | `/panel` | Panel HTML entry point. |
| `GET` | `/panel/assets/{path}` | Panel static assets. |
| `GET` | `/panel/api/threads` | List threads. |
| `POST` | `/panel/api/threads` | Create thread. |
| `GET` | `/panel/api/threads/{thread_id}` | Get thread. |
| `DELETE` | `/panel/api/threads/{thread_id}` | Delete thread. |
| `POST` | `/panel/api/chat` | Process chat. |
| `GET` | `/panel/api/bookmarks` | List bookmarks. |
| `POST` | `/panel/api/bookmarks` | Save bookmark. |
| `GET` | `/panel/api/widgets` | List widgets. |
| `POST` | `/panel/api/widgets` | Save widget. |
| `GET` | `/panel/api/graph` | Get graph. |
| `POST` | `/panel/api/graph` | Save graph. |
| `GET` | `/notifications` | List notifications. |
| `PATCH` | `/notifications/{id}/read` | Mark notification read. |
| `PATCH` | `/notifications/read-all` | Mark all notifications read. |
| `DELETE` | `/notifications/all` | Delete all notifications. |

Some tests reference `/panel/api/graphs`; current CLI server route is `/panel/api/graph`.

## Settings and Integrations

| Method | Path | Description |
|---|---|---|
| `GET/POST` | `/api/settings/cloud-node` | Cloud node settings. |
| `GET/POST` | `/api/settings/discord` | Discord settings. |
| `POST` | `/api/settings/discord/test` | Test Discord connection. |
| `GET/POST` | `/api/settings/telegram` | Telegram settings. |
| `POST` | `/api/settings/telegram/test` | Test Telegram connection. |
| `GET` | `/plugins/health` | Plugin health, when enterprise routes are enabled. |
| `POST` | `/plugins/sync` | Plugin sync, when enterprise routes are enabled. |

## Proxy and OpenAI-Compatible Endpoints

| Method | Path | Description |
|---|---|---|
| `POST` | `/v1/embeddings` | OpenAI-style embeddings response for one input. |
| `POST` | `/v1/auth/session` | Create an auth/session context. |
| `POST` | `/v1/proxy/chat/completions` | Chat completion proxy. |
| `POST` | `/v1/proxy/chat/completions/batch` | Batch chat proxy. |
| `POST` | `/v1/proxy/request` | Generic proxy request. |

Embedding request:

```json
{"input": "hello world", "model": "all-MiniLM-L6-v2"}
```
