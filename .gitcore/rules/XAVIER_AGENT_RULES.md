# 🤖 Xavier Core Integration Guidelines for Coding Agents (Cursor, Windsurf, Cline)
> **Mandatory System Directive**: This document defines the protocol for AI agents to interact with the Xavier Context Engine during engineering tasks.

---

## 🧭 The Core Principle: "Recall, Analyze, Persist"
When working in this codebase, you are not acting in isolation. You have access to a shared, persistent cognitive memory graph and a static AST code graph. You must follow the three-stage lifecycle for every task:

```
┌────────────────────────────────┐
│   1. RECALL (Pre-Task)         │ ──▶ Query Xavier Memory for historical decisions
└────────────────────────────────┘
               │
               ▼
┌────────────────────────────────┐
│   2. ANALYZE (Static Graph)    │ ──▶ Map symbols & dependencies via /code/
└────────────────────────────────┘
               │
               ▼
┌────────────────────────────────┐
│   3. PERSIST (Post-Task)       │ ──▶ Store verified state and devlog in memory
└────────────────────────────────┘
```

---

## 📡 Essential Protocols & Endpoints

All HTTP requests to Xavier must target `http://localhost:8006` (or the port defined by `$XAVIER_PORT`) and **MUST** include the secure authorization header:
`X-Xavier-Token: <value_from_XAVIER_TOKEN_env>`

### 🔍 Stage 1: Pre-Task Recall (Semantic RAG)
Before modifying code or writing plans, search Xavier to find if this module was touched before, what decisions were made, and what guidelines apply.

* **Endpoint**: `POST /memory/search`
* **Payload**:
  ```json
  {
    "query": "authentication security dev-token stability",
    "limit": 5
  }
  ```
* **Directive**: Extract files, architectural specs, or prior bugs associated with the keywords in your current goal.

---

### 🕸️ Stage 2: Codebase Structural Mapping (Static Code Graph)
Do not use raw text search or RAG vector search to find code files, functions, or dependencies. Use the tree-sitter static graph endpoints to obtain structural precision.

#### A. Locate Symbols (Fuzzy Search)
* **Endpoint**: `POST /code/find`
* **Payload**: `{"query": "resolve_xavier_token"}`

#### B. Map AST and File Context
* **Endpoint**: `POST /code/context`
* **Payload**: `{"query": "src/security/auth.rs"}`
* **Returns**: High-level AST structure, exported functions, structs, classes, methods, and active imports.

#### C. Trace Dependency Graph
* **Endpoint**: `POST /code/dependencies`
* **Payload**: `{"query": "src/server/mcp_server.rs", "depth": 2}`
* **Returns**: Inbound import links to prevent circular dependencies.

---

### 💾 Stage 3: Atomic State Persistence (Durable Retention)
Once your task is completed, compiling successfully, and verified via local tests:
1. Formulate a short, markdown-formatted summary of your modifications and *why* they were made.
2. Ingest this summary into Xavier's durable memory so that subsequent agent sessions have immediate context.

* **Endpoint**: `POST /memory/add`
* **Payload**:
  ```json
  {
    "path": "tasks/verification/<issue_number_or_slug>",
    "content": "### Task: Unify settings authentication\n- Removed dev-token and XAVIER_DEV_MODE fallbacks.\n- Hardened auth.rs to strictly require XAVIER_TOKEN env.\n- Added tests/storage_isolation.rs for multi-tenant verification.",
    "metadata": {
      "type": "task_verification",
      "status": "completed",
      "author": "your_agent_name"
    }
  }
  ```

---

## 💡 Accessing IDE Agent Chat History
The local **Agentic Scanner Daemon** runs continuously in the background on the Xavier server. It automatically discovers, parses, and vector-indexes your own chat history with IDE agents (Cursor, Windsurf, VS Code, Kiro).

If you are continuing a task previously discussed in a Cursor composer or Windsurf chat session:
* Search specifically for historical chat fragments by querying:
  * `query: "Cursor chat history auth"` or `query: "Windsurf Handoff"`
* Look for virtual documents with path prefix `agent_memory://` to retrieve absolute transcripts of prior agent reasoning!

---

## ⚠️ Absolute Agent Constraints
1. **Mandatory Token Header**: Never omit `X-Xavier-Token`. All requests without this header will be rejected with `401 Unauthorized` due to our hardened security policies.
2. **Never Commit Secrets**: Never store API keys, database passwords, or plain token values inside Xavier memory.
3. **No Redundant RAG**: Do not index raw code files inside the semantic vector memory store. The code graph does this with 100% precision via AST parsing. Use vector memory ONLY for documentation, specs, chat logs, and task summaries.
