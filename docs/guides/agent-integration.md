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
# Start Xavier daemon via canonical or backwards-compatible serve command
xavier serve --host 0.0.0.0 --port 8006

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

## 4. Gestalt Inbound Plugin & Event Bus Contract

Gestalt agents interface with Xavier through the inbound plugin adapter (`src/adapters/inbound/gestalt/mod.rs`). The event bus bridge (`GestaltAdapter`) subscribes to `XavierEventBus` to receive and emit real-time agent execution states:

* **Task Lifecycle Events:** `AgentTaskStarted`, `AgentTaskCompleted`, `AgentTaskFailed`
* **Gestalt Protocol Events:** `MemoryFragmentSaved`, `TaskStateUpdated`, `TelemetryPing`
* **CLI Server Interface:** Supports `xavier serve --host <HOST> --port <PORT>` and `xavier http --http` for continuous agent daemon execution.

## 5. MCP Tools Reference

If integrating via Model Context Protocol (stdio or SSE on `:8100`):

| Tool | Purpose |
|---|---|
| `mem_search` / `memory_search` | Progressive disclosure candidate search (`page`, `limit`, `total_pages`, `has_more`, scores, snippets, provenance). |
| `mem_context` / `memory_context` | Budget-bounded memory context block with character/token estimation. |
| `save_fragment` / `memory_save` | Ingest memory fragments into the vector store with structured provenance & namespaces. |
| `xavier_context_save` | Save snapshot of session state across workspace pools. |
| `xavier_context_restore`| Rehydrate session context within token limits with auto-resurrection. |
| `xavier_issue_context_package` | Formulate a GitHub issue context bundle. |
| `codegraph_explore` / `trace_path` | Persistent CodeGraph AST symbol exploration & dependency tracing. |

---

## 6. Universal Agent Importer Architecture

To guarantee comprehensive observability and cross-agent context sharing across the SWAL mesh, Xavier interfaces directly with heterogeneous agent runtimes:

```
┌─────────────────────────┐  ┌─────────────────────────┐  ┌─────────────────────────┐
│     Hermes Sessions     │  │   Google Antigravity    │  │        OpenCode         │
│  ~/.hermes/sessions/*.json  │  │   ~/.gemini/antigravity │  │ ~/.local/share/opencode │
│  (HermesImporter)       │  │   (AntigravityImporter) │  │ (OpenCodeImporter)      │
└────────────┬────────────┘  └────────────┬────────────┘  └────────────┬────────────┘
             │                            │                            │
             └───────────────────┬────────┴────────────────────────────┘
                                 ▼
                 ┌───────────────────────────────┐
                 │   Xavier Agent Session VFS    │
                 │   - Semantic compression      │
                 │   - ANSI / Base64 noise strip │
                 │   - Structured checkpointing  │
                 │   - Provenance attribution    │
                 └───────────────┬───────────────┘
                                 ▼
                     SQLite-vec & BM25 Storage
```

### Supported Transcripts & Format Matrix:
1. **Hermes:** Structured JSON transcripts with tool call trees and session events.
2. **Google Antigravity:** JSONL step streams (`~/.gemini/antigravity/brain/<id>/.system_generated/logs/transcript.jsonl`) capturing `USER_INPUT`, `PLANNER_RESPONSE`, and tool executions.
3. **OpenCode:** Local SQLite stores (`~/.local/share/opencode/opencode.db` / `history.jsonl`).
4. **OpenClaw & Codex:** Workspace task chronologies and incremental session diffs.

---

## 7. Anti-Patterns & Best Practices

| Anti-Pattern | Recommended Practice |
|---|---|
| ❌ Storing high-frequency ephemeral logs in memory | ✅ Ingest only condensed summaries, decisions, and outcomes. |
| ❌ Querying unbudgeted full memories per turn | ✅ Use progressive disclosure (`page`, `limit`) with a token budget. |
| ❌ Hardcoding tokens in source code | ✅ Export `XAVIER_TOKEN` in environment files. |
| ❌ Ingesting duplicate records without path/ID | ✅ Assign canonical paths (e.g. `decisions/YYYY-MM-DD/...`). |

