# HTTP REST & WebSocket API Reference

The Xavier daemon provides a RESTful HTTP service on port 8006 (default) and Server-Sent Events (SSE) / WebSocket endpoints for realtime agent streaming.

---

## 1. Authentication

All requests require the `Authorization` header with a valid bearer token matching `XAVIER_TOKEN`:
```http
Authorization: Bearer <XAVIER_TOKEN>
```

---

## 2. Core Endpoints

### Health Check
- **`GET /health`**
  - Response: `200 OK`
  - Body: `{"status": "healthy", "version": "0.0.1", "database": {"status": "ok"}}`

### Search Memories
- **`POST /v1/memories/search`**
  - Body: `{"query": "string", "limit": 10, "mode": "hybrid" | "vector" | "keyword"}`

### Store Memory
- **`POST /v1/memories`**
  - Body: `{"content": "string", "kind": "episodic" | "fact", "metadata": {}}`

### Mesh Node Status
- **`GET /v1/mesh/status`**
  - Response: `{"node_id": "...", "peers_connected": 3, "sync_state": "synced"}`

### Code Graph Query
- **`POST /v1/codegraph/query`**
  - Body: `{"action": "find_symbol" | "find_callers", "target": "string"}`
