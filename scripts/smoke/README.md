# Xavier contract smoke

## MCP / HTTP contract

```bash
./scripts/smoke/mcp_contract.sh
```

Requires a reachable Xavier HTTP API on `:8006` (and `XAVIER_TOKEN` from repo `.env`). Optionally probes MCP HTTP on `:8100` for Origin checks; skips if the port is closed.

Checks:

1. MCP stdio (`scripts/mcp/xavier-mcp-cursor.sh` or `xavier mcp`): initialize + `tools/list`; first stdout line is JSON (`{`); required tools `mem_search`, `memory_context`, `create_memory`; optional `memory_search` call without error.
2. `POST /memory/search` → 200 with token, 401 with wrong token.
3. If `:8100` is up: bare Origin `localhost` → 403; trusted Origin may return 200.
