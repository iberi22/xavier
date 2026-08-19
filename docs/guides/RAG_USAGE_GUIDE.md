# RAG Usage Guide for AI Agents

Xavier provides a powerful Retrieval-Augmented Generation (RAG) backend through its Model Context Protocol (MCP) server. This allows AI agents to seamlessly store, search, and retrieve knowledge using hybrid search strategies.

## Canonical agent loop

1. **`mem_search`** — Fat index (candidates with snippets; no full body by default)
2. **`memory_context`** or **`get_memory`** — Page-in selected ids
3. **`create_memory`** — Persist new knowledge

Prefer these names. Aliases `search_memory` / `memory_search` still work but are deprecated; `mem_context` aliases `memory_context`.

## Connection Methods

### 1. Stdio Transport (Local)
Ideal for local AI clients like Claude Desktop, Cursor, or Windsurf. The client spawns Xavier as a subprocess and communicates via standard input/output.

**Configuration Example (Claude Desktop):**
```json
{
  "mcpServers": {
    "xavier": {
      "command": "xavier",
      "args": ["mcp"],
      "env": {
        "XAVIER_TOKEN": "your-secret-token"
      }
    }
  }
}
```

### 2. HTTP+SSE Transport (JSON-RPC MCP — canonical)
Used for remote agents or distributed architectures. Default MCP port is **8100** (not the main REST API on :8006).

- **POST `/mcp`**: JSON-RPC requests
- **GET `/mcp`**: SSE notification stream

Start the MCP HTTP server:
```bash
xavier mcp --port 8100
# or alongside the main HTTP API:
xavier http --mcp-port 8100
```

**Do not confuse** with legacy REST `GET http://localhost:8006/mcp/tools` (deprecated listing).

## Available RAG Tools

### `mem_search` (canonical)
Primary fat-index search. Hybrid BM25 + vector + RRF. Returns structured candidates `{id,path,score,snippet,kind}`. Set `include_content=true` only when necessary; prefer paging in via `memory_context` with ids.

**Parameters:**
- `query` (string, required): The search terms or question
- `limit` (number, optional): Max results (default: 10)
- `include_content` (boolean, optional): Full body per candidate (default: false)
- `filters` (object, optional): Namespace / kind filters

Deprecated aliases with the same structured handler: `search_memory`, `memory_search` (also accepts legacy `namespace`).

### `memory_save`
Stores new documents or knowledge atoms.
- **Automatic Embedding**: Xavier automatically generates vectors for saved text if an embedding provider is configured.
- **Metadata**: Attach free-form JSON for later filtering.

### `memory_context` / `mem_context`
Returns a formatted context block (or structured page-in) for selected memory ids or a query — ready for LLM injection. Prefer ids from `mem_search`.

### `create_memory` / `get_memory`
Write and read durable documents by path/id.

### `save_fragment` / `search_fragments`
Optimized for "Episodic Memory." Agents can save small observations, thoughts, or interaction fragments.
- Includes `importance` scoring (0.0 to 1.0).
- Supports `tags` and `context` identifiers.

## Search Strategies

Xavier's RAG engine supports multiple retrieval modes depending on the query and configuration.

### 1. Lexical (BM25)
Best for exact matches, technical terms, or specific IDs.
- *Internal Logic*: Uses a high-performance Rust implementation of BM25.

### 2. Semantic (Vector)
Best for conceptual queries where exact words might not match.
- *Internal Logic*: Uses `sqlite-vec` for efficient local vector storage.

### 3. Hybrid (RRF)
The default mode. It combines Lexical and Semantic results using **Reciprocal Rank Fusion (RRF)**.
- **RRF K=60**: Standard constant for balancing ranks.
- **Weights**: Default balance is 70% Keyword / 30% Semantic, adjustable in `config/xavier.config.json`.

## Configuration Recommendations

### Embedding Providers
Xavier supports multiple embedding backends:
- **Local (GLLM)**: High privacy, zero cost. Optimized for AMD/NVIDIA GPUs via GLLM.
- **Cloud (OpenRouter / OpenAI)**: High-quality embeddings via cloud APIs.
  - **OpenRouter**: Supported via `XAVIER_EMBEDDING_URL` and `XAVIER_EMBEDDING_MODEL`.
  - **Fallback**: Cloud mode automatically falls back to local GLLM if the cloud provider is unavailable.

### Cloud Mode & Fallbacks
When `XAVIER_EMBEDDING_PROVIDER_MODE=cloud` is set, Xavier prioritizes the configured cloud endpoint.
- **Configurable Timeout**: Use `XAVIER_EMBEDDING_TIMEOUT_SECS` (default: 30) to adjust API wait times.
- **Offline Resilience**: If the cloud API fails or the system is offline, Xavier will attempt to generate embeddings using the local GLLM backend as a secondary fallback.

### Thresholds and Weights
You can fine-tune retrieval behavior in `config/xavier.config.json`:
- `KEYWORD_WEIGHT`: Increase for technical documentation.
- `SEMANTIC_WEIGHT`: Increase for conversational or "fuzzy" knowledge bases.

## Namespacing & Isolation
To prevent context contamination between different agents or projects, use `filters` on `mem_search` (or legacy `namespace` on `memory_search`):

```json
{
  "query": "deployment steps",
  "filters": {
    "project": "frontend-v2",
    "agent_id": "deploy-bot"
  }
}
```
This ensures the RAG results are strictly scoped to the relevant context.
