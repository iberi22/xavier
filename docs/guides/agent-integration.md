# Agent Integration Guide — Xavier Context & Memory Engine

This guide details how autonomous agents (Hermes, Gestalt, Jules, OpenClaw, Antigravity) connect to and leverage **Xavier** as their centralized memory and shared context provider.

---

## 1. Core Architecture & Mental Model

Xavier provides low-latency contextual recall over persistent vector stores (`sqlite-vec`), keyword indices (`BM25`), hierarchical graph clusters, and code symbols.

```
┌────────────────────────────────────────────────────────┐
│                   External Agent                       │
│        (Hermes / Gestalt / Jules / Antigravity)        │
└──────────────┬──────────────────────────┬──────────────┘
               │ HTTP REST (:8006)        │ MCP (:8100 / stdio)
               ▼                          ▼
┌────────────────────────────────────────────────────────┐
│                        XAVIER                          │
│  ┌───────────────────────┐  ┌───────────────────────┐  │
│  │   /v1/context/package │  │      MCP Tools        │  │
│  │  (One-shot fat search)│  │   (36 tool catalog)   │  │
│  └───────────┬───────────┘  └───────────┬───────────┘  │
│              └─────────────┬────────────┘              │
│                            ▼                           │
│               SQLite-vec / BM25 / Graph                │
└────────────────────────────────────────────────────────┘
```

---

## 2. Authentication & Prerequisites

Every authenticated request to Xavier requires the `X-Xavier-Token` header matching the server's `XAVIER_TOKEN` environment variable.

```bash
export XAVIER_URL="http://localhost:8006"
export XAVIER_TOKEN="your-xavier-token"
```

Verify service connectivity:

```bash
curl -s "$XAVIER_URL/health" | jq .status
# Output: "healthy"
```

---

## 3. Session Lifecycle & Workflow (PRE / RUN / POST)

Agents operating within the SWAL ecosystem MUST follow the standard lifecycle:

### Step 1: PRE — Context Assembly

Before taking action, assemble relevant prior decisions, architecture constraints, and past sessions:

```bash
curl -s -X POST "$XAVIER_URL/v1/context/package" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "integracion wave10 context package",
    "max_tokens_budget": 1024,
    "limit": 5,
    "kinds": ["decision", "document"]
  }'
```

### Step 2: EXECUTION — In-session Search

If intermediate context or symbol lookup is required:

```bash
curl -s -X POST "$XAVIER_URL/v1/memories/search" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "sqlite pragma integrity_check",
    "limit": 3
  }'
```

### Step 3: POST — Memory Ingestion & Session Closure

Upon completing a milestone or recording an architectural decision (ADR):

```bash
curl -s -X POST "$XAVIER_URL/v1/memories" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "text": "Completed wave10 context packaging endpoint with char-budget estimation and integrity check honest status propagation.",
    "user_id": "decisions/2026-08-29/wave10-context-package",
    "kind": "decision",
    "metadata": {
      "agent": "antigravity",
      "wave": "wave10",
      "status": "verified"
    }
  }'
```

---

## 4. MCP Tools Reference

If integrating via Model Context Protocol (stdio or SSE on `:8100`):

| Tool | Purpose |
|---|---|
| `mem_search` | Candidate search with scores, snippets, and provenance metadata. |
| `mem_context` | Budget-bounded memory context block. |
| `mem_add` | Ingest memory fragments into the vector store. |
| `xavier_context_save` | Save snapshot of session state. |
| `xavier_context_restore`| Rehydrate session context within token limits. |
| `xavier_issue_context_package` | Formulate a GitHub issue context bundle. |

---

## 5. Anti-Patterns & Best Practices

| Anti-Pattern | Recommended Practice |
|---|---|
| ❌ Storing high-frequency ephemeral logs in memory | ✅ Ingest only condensed summaries, decisions, and outcomes. |
| ❌ Querying unbudgeted full memories per turn | ✅ Use `/v1/context/package` or snippet mode with a token budget. |
| ❌ Hardcoding tokens in source code | ✅ Export `XAVIER_TOKEN` in environment files. |
| ❌ Ingesting duplicate records without path/ID | ✅ Assign canonical paths (e.g. `decisions/YYYY-MM-DD/...`). |
