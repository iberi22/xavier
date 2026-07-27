---
name: xavier-memory-protocol
description: >-
  Canonical Fat Search → Page-In → Persist loop for Xavier durable memory over
  MCP or HTTP. Use before complex tasks (recall) and after discoveries (persist).
  Prefer this skill over cortex-memory / agentic-memory-ops for the mandatory loop.
---

# Xavier Memory Protocol (canonical)

Mandatory turn-based memory lifecycle for every agent working in this repo:

```
[ Pre-Task Recall ]  →  [ Task Execution ]  →  [ Post-Task Retention ]
   Fat Search             Implement            Persist discoveries
   Page-In only what matters
```

Xavier is the sole durable memory (`http://localhost:8006`, MCP server `xavier-memory`). Cortex is deprecated and removed.

## Progressive disclosure (token savings)

Do **not** dump full memory bodies into the prompt by default.

1. **Fat Search** — metadata + scores + short snippets only (`include_content` false / omitted).
2. **Page-In** — fetch full text **only** for the IDs/paths you need.
3. **Persist** — store durable decisions, facts, bugs, and architectural learnings (never secrets or ephemeral scratch).

Estimate tokens conservatively as `ceil(chars / 4)`. Prefer a wide fat index over loading whole documents.

Related deep dive: `docs/TOKEN_SAVINGS_ANALYSIS.md`.

---

## MCP path (preferred when tools are available)

| Step | Tool | Role |
|------|------|------|
| 1. Fat Search | `mem_search` | Candidates: `id`, `path`, `score`, `snippet`, `kind` |
| 2. Page-In | `memory_context` and/or `get_memory` | Full body for selected ids/paths |
| 3. Persist | `create_memory` | Write durable knowledge |

### Examples

```text
mem_search(query="<task reformulated>", limit=5, filters={project: "<project>"})
memory_context(ids=["01H…", "01H…"])   # or get_memory(path="…") / get_memory(id="…")
create_memory(path="<descriptive-slug>", content="<result>", kind="decision|fact|task|bug")
```

Auth for MCP stdio is usually inherited from the process env (`XAVIER_TOKEN` loaded by the launcher). Do not hardcode tokens in `mcp.json`.

### Deprecated / legacy MCP aliases

Prefer the canonical names above. Hosts may still expose these aliases — treat them as compatibility shims, not the primary API:

| Alias | Prefer instead |
|-------|----------------|
| `search_memory` | `mem_search` |
| `memory_search` | `mem_search` |
| `memory_save` | `create_memory` |

---

## HTTP path (when MCP is unavailable)

Base URL: `http://localhost:8006` (default REST).

**Headers (required for authenticated routes):**

```http
Content-Type: application/json
X-Xavier-Token: <value from $XAVIER_TOKEN / .env>
```

Never hardcode the token in docs, skills, or committed config. Read it from the environment.

| Step | Endpoint | Role |
|------|----------|------|
| 1. Fat Search | `POST /memory/search` | Metadata-oriented recall |
| 2. Page-In | Fetch specific memory by id/path (e.g. `GET`/`POST` memory get routes used by your client) | Full body only for selected hits |
| 3. Persist | `POST /memory/add` | Store durable content |

### Fat Search

```bash
curl -sS -X POST http://localhost:8006/memory/search \
  -H "Content-Type: application/json" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -d '{"query":"<task>","limit":5}'
```

### Persist

```bash
curl -sS -X POST http://localhost:8006/memory/add \
  -H "Content-Type: application/json" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -d '{"path":"<slug>","content":"<finding>","kind":"decision"}'
```

HTTP search responses may include fuller records than MCP fat search. Still apply progressive disclosure: skim hits, then page in only what you need for the current turn.

---

## Ports & transports (do not confuse)

| Surface | Default | Notes |
|---------|---------|--------|
| REST API | `:8006` | `xavier serve` / `xavier http` |
| MCP JSON-RPC (HTTP+SSE) | `:8100` | Enabled via `xavier http --mcp-port 8100` (default when MCP HTTP is on; `0` disables) |
| Legacy REST MCP bridge | `:8006/mcp` | Older clients; prefer stdio or `:8100` |
| MCP stdio | — | `xavier mcp` (Cursor/Claude via wrapper) |

Remote/network MCP for IDE hosts that speak HTTP MCP: run `xavier http --mcp-port 8100` (not `xavier mcp --port`).

Cursor local stdio launcher: `scripts/mcp/xavier-mcp-cursor.sh` — see `docs/guides/MCP_INTEGRATION.md`.

---

## Related skills

- **Advanced RAG / context budgets:** `.agents/skills/agentic-memory-ops/SKILL.md` (builds on this loop).
- **Legacy MCP transport notes:** `.agents/skills/cortex-memory/SKILL.md` (historical folder name; Cortex product removed).

Index: `.agents/skills/README.md`.
