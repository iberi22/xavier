---
title: Getting Started with Xavier
description: Quick start, installation, binary execution and systemd service deployment.
---

# Getting Started with Xavier

Xavier is the cognitive memory system for AI Swarms and autonomous coding agents.

## Quick Installation

```bash
# Build and install locally from source
cargo install --path . --locked
```

## Running the Server

```bash
# Start HTTP REST API (:8006) and MCP server (:8100)
xavier http

# Start MCP stdio server for Claude Desktop / Cursor / Hermes
xavier mcp
```
