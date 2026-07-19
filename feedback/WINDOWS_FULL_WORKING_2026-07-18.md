# Windows full-working + Graph Explorer — 2026-07-18

## Smoke (live)

| Endpoint | Status |
|----------|--------|
| `GET /health` | 200 |
| `GET /memory/graph/view` | 200 |
| `GET /memory/graph/entities` | 200 (528 entities) |
| `GET /code/graph/view` | 200 |
| `GET /code/stats` | 200 (955 Rust symbols) |
| `GET/POST /panel/api/graph` | 200 (CRUD persists) |
| `POST /memory/search` | 200 |
| `POST /memory/add` | 200 |

**Process:** `target/release/xavier.exe` PID listening on `0.0.0.0:8006`  
**Data:** `%APPDATA%\xavier\vec-store.sqlite3`  
**Start:** `scripts/start-xavier-windows.ps1`  
**Install alias:** `C:\Users\belal\bin\xavier-ola-graph.exe` (`bin\xavier.exe` often locked by Grok MCP respawn)

## Fixes landed this session

1. **Null embeddings** — `deserialize_record` accepts `Option<Vec<u8>>` (model change invalidation).
2. **panel_graphs project_id** — panel used `"vec_store"`; store uses `vec_store_{sha12(path)}`. Aligned.
3. **Legacy entities.workspace_id** — V4 indexes failed on old `entities` tables; repair columns before V4_UP.
4. **code_graph FTS** — rebuild `symbols_fts` when missing `stable_id`.
5. **vec_f32 / memory add** — never open vec pool before `sqlite3_auto_extension`.

## Residuals

- MCP `tools/list` → Unexpected response type (prefer HTTP).
- Embedding bulk reindex after model change (many NULL embeddings until reindex).
- Optional `docs/GRAPH_LAYERS.md`.
- Health `database.page_count=0` probe noise (degraded OK for smoke).

## Memory ids

- Status: `01KXSSRC7XASNWV97RZ6D23N9J` (`status/windows-full-working-graph-ola-2026-07-18`)
- Smoke add: `01KXSSNZJ85W3KZHS92QJVNTGA`
- Plan (prior): `01KXSNW3EHZ0F5PQA6358NPKF7`
