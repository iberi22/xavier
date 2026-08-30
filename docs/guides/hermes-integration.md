# Hermes Agent Integration Guide — Xavier Plugin & Memory Provider

This guide details the integration configuration for **Hermes Agent** as the primary consumer of Xavier's vector memory and context runtime.

---

## 1. Hermes Plugin Configuration

Hermes uses a plugin located at `~/.hermes/plugins/xavier/__init__.py` and configures Xavier as its external memory provider in `~/.hermes/config.yaml`.

```yaml
memory:
  provider: xavier
  base_url: "http://localhost:8006"
  timeout_secs: 7
  token: "${XAVIER_TOKEN}"

mcp_servers:
  xavier:
    url: "http://localhost:8100/sse"
    enabled: true
```

---

## 2. Latency & Timeout Guidelines

Measured production latencies for typical Xavier operations:

| Operation | Typical Latency | Configured Timeout | Notes |
|---|---|---|---|
| `GET /health` | ~1.2ms | 2s | Fast internal status probe |
| `POST /v1/context/package` | ~18-45ms | 5s | Includes vector similarity & snippet pruning |
| `POST /v1/memories` | ~25-80ms | 7s | Embedding generation + SQLite transaction |
| `MCP tool invocation` | ~30-90ms | 10s | JSON-RPC over HTTP/SSE |

> **Important**: Ensure `timeout_secs >= 7` in Hermes configuration to account for model inference latency during background memory embeddings.

---

## 3. Environment & Token Sharing

The `XAVIER_TOKEN` defined in `~/.hermes/.env` must match the token used when starting Xavier (`/proc/PID/environ` check):

```bash
# Verify matching token in environment
grep "XAVIER_TOKEN" ~/.hermes/.env
grep "XAVIER_TOKEN" .env
```

---

## 4. Common Diagnostics & Error Resolution

| Hermes Log Error | Root Cause | Remediation |
|---|---|---|
| `401 Unauthorized: Invalid or missing X-Xavier-Token` | Token mismatch between Hermes config and Xavier daemon | Align `XAVIER_TOKEN` in both `.env` files and restart daemon. |
| `ReadTimeoutError: request timed out after 5.0s` | Complex vector search with large batch embedding | Increase timeout to `7s` in `config.yaml`. |
| `ConnectionRefusedError: [Errno 111]` | Xavier server is not running on port 8006 | Start Xavier service via `xavier http`. |
| `MCP connection failed` | MCP HTTP endpoint not enabled | Start Xavier with `xavier http` (enables `:8100` SSE by default). |

---

## 5. Related Documentation

- [Agent Integration Guide](agent-integration.md) — Generic agent connection guide.
- [API Reference](../site/src/content/docs/reference/api.md) — Full REST endpoint definitions.
