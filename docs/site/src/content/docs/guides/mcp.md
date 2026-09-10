---
title: MCP Integration
description: Using Xavier with Model Context Protocol
---

# MCP Integration Guide

Xavier supports the Model Context Protocol (MCP) for seamless integration with AI clients.

## Supported Clients

- **Claude Desktop** (Windows, macOS, Linux)
- **Cursor**
- **Windsurf**
- **Other MCP-compatible clients**

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

## Available MCP Tools

| Tool | Description |
|------|-------------|
| `mem_search` / `memory_search` | Vector & hybrid search over cognitive memories |
| `mem_save` / `memory_add` | Add a new memory to the store |
| `mem_stats` / `memory_stats` | Get memory and storage statistics |
| `code_find` | Search code AST symbols by name, query, or kind |
| `code_scan` | Trigger zero-token AST indexing of local repository |
| `sys_health` | System health probe and resource status |

## Migration & Connection Modes

Xavier supports two connection modes for MCP:

1. **Stdio Mode (Local CLI)**: Executed directly via `xavier mcp`. Ideal for local IDEs (Cursor, Claude Desktop).
2. **SSE Mode (HTTP Transport)**: Started via `xavier http` with native SSE endpoints served at `http://127.0.0.1:8006/mcp/sse`. Ideal for remote agents and service-to-service MCP connections.

## Usage

Once configured, you can ask Claude things like:

- "Search my memory for architecture decisions"
- "Remember that I prefer tabs over spaces"
- "Find the code that handles authentication"

Xavier will be consulted automatically for relevant context.

## Security

All MCP requests are subject to the same security scanning as HTTP requests. Prompt injection attempts will be blocked automatically.

## Troubleshooting

### Connection Issues

1. Verify Xavier is running: `curl http://localhost:8006/health`
2. Check token matches between config and environment

### Performance

- Vector search is ~7ms average
- First request may be slower due to connection initialization
