# Model Context Protocol (MCP) Integration

Xavier exposes a standard Model Context Protocol (MCP) interface allowing IDEs, agents, and LLMs to query and store memories seamlessly.

---

## 1. Supported Clients

- **Claude Desktop**
- **Cursor IDE**
- **OpenCode**
- **Hermes Agent**
- **VSCode (via Claude/Cline extensions)**

---

## 2. Configuration Examples

### Claude Desktop (`claude_desktop_config.json`)
```json
{
  "mcpServers": {
    "xavier": {
      "command": "xavier",
      "args": ["mcp"],
      "env": {
        "XAVIER_TOKEN": "your-token-here",
        "XAVIER_DATA_DIR": "~/.local/share/xavier"
      }
    }
  }
}
```

### Cursor (`.cursor/mcp.json`)
```json
{
  "mcpServers": {
    "xavier-memory": {
      "command": "xavier",
      "args": ["mcp"]
    }
  }
}
```

### Hermes Agent (`~/.hermes/config.yaml`)
```yaml
memory:
  provider: "xavier"
  endpoint: "http://localhost:8006"
  token: "${XAVIER_TOKEN}"
```

---

## 3. Migration & Connection Modes

Xavier supports dual transport modes for MCP integration:

- **Stdio Transport**: Started via `xavier mcp`. Reads standard input/output using JSON-RPC 2.0.
- **SSE Transport (HTTP)**: Served via `xavier http` at `/mcp/sse`.

### Tool Catalog

- `mem_search` (`memory_search` alias): Hybrid/vector recall across memories.
- `mem_save` (`memory_add` alias): Persist conceptual memory record.
- `code_find`: Query AST code symbols (functions, structs, routes, variables) with FTS5 and symbol kind filters.
- `sys_health`: Monitor daemon health, system load, and storage usage.
