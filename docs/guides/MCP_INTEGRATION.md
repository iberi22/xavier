# MCP Integration Guide

Xavier supports the Model Context Protocol (MCP) for seamless integration with AI clients.

## Supported Clients

- **Claude Desktop** (Windows, macOS, Linux)
- **Cursor**
- **Windsurf**
- **Other MCP-compatible clients**

## RAG Capabilities

AI agents can use Xavier via MCP as a Retrieval-Augmented Generation (RAG) backend. For detailed information on tools and strategies, see the [RAG Usage Guide](./RAG_USAGE_GUIDE.md).

## Ports — do not confuse

| Service | Port | Notes |
|---------|------|-------|
| Main HTTP API (REST memory, health) | **:8006** | `POST /memory/search`, etc. |
| **JSON-RPC MCP (canonical)** | **:8100** | `xavier mcp` / `xavier http --mcp-port` — `POST /mcp` |
| Legacy REST tool list | `:8006/mcp/tools` | **Deprecated** — response includes `deprecated: true`; use JSON-RPC on :8100 |

See also [MCP Contract](../MCP_CONTRACT.md).

## Setup

### 1. Configure Claude Desktop

Add to your MCP settings file:

**Windows:** `%APPDATA%\Claude\claude_desktop_config.json`
**macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`

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

### 2. Restart Claude Desktop

After saving the configuration, restart Claude Desktop.

## Available MCP Tools (canonical)

Agent loop: **`mem_search` → `memory_context` / `get_memory` → `create_memory`**

| Tool | Description |
|------|-------------|
| `mem_search` | Fat index search (structured candidates; prefer over aliases) |
| `memory_context` | Page-in full/partial content by ids or query |
| `get_memory` | Fetch one memory by id |
| `create_memory` | Persist a new memory document |
| `memory_save` | Save free-form text with optional namespace |
| `health_check` | Structured health + tool count |

Deprecated aliases (still listed for compat): `search_memory`, `memory_search`. Alias: `mem_context` → `memory_context`.

## Usage

Once configured, you can ask Claude things like:

- "Search my memory for architecture decisions"
- "Remember that I prefer tabs over spaces"
- "Find the code that handles authentication"

Xavier will be consulted automatically for relevant context.

## Security

All MCP requests are subject to the same security scanning as HTTP requests. Prompt injection attempts will be blocked automatically.


## Contract smoke

Run the contractual MCP/HTTP smoke (stdio JSON purity, required tools, `/memory/search` auth, optional `:8100` Origin):

```bash
./scripts/smoke/mcp_contract.sh
```

See also `scripts/smoke/README.md`. Sources repo `.env` for `XAVIER_TOKEN` (never prints secrets). Exit code is non-zero on failure.

## Troubleshooting

### Connection Issues

1. Verify main API health: `curl http://localhost:8006/health`
2. Verify MCP JSON-RPC: `curl -X POST http://localhost:8100/mcp -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'`
3. Check token matches between config and environment

### Performance

- Vector search is ~7ms average
- First request may be slower due to connection initialization
