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
