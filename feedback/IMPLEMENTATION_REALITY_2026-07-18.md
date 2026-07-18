# Implementation Reality Report — 2026-07-18

## MCP Xavier

| Item | Result |
|------|--------|
| Configure Grok MCP `xavier-memory` | **Done** (`grok mcp add` → `~/.grok/config.toml`) |
| `grok mcp doctor` | handshake OK; **tools/list Unexpected response type** (protocol drift Grok ↔ Xavier MCP stdio) |
| HTTP memory | **Working** — plan stored id `01KXSNW3EHZ0F5PQA6358NPKF7` |
| MCP on live session | Host still may need **session restart** to load new server tools |

## Test battery (local)

| Suite | Result | Notes |
|-------|--------|-------|
| panel-ui `pnpm test` | **18/18 PASS** | adapters, roadmap, a11y, auth, badge |
| panel-ui typecheck | **PASS** | |
| panel-ui build | **PASS** | |
| `cargo check -p xavier --features ci-safe` | **PASS** | |
| cargo test `graph` / `panel` / `entity_graph` | **PASS** | |
| cargo test storage migrations | **PASS** (+ new legacy `name` column upgrade test) |
| cargo test full lib ci-safe | **1104 pass / 18 fail / 5 ignored (98.4%)** | Failures cluster: MCP tools, settings env, embedding cloud mocks |

### Full-lib failures (pre-existing / not Graph-wave)

- MCP progressive / token-savings regressions (8)
- embedding auto/cloud probe (3)
- health embedding alert (2)
- settings apply_to_env / load (5)

## Graph feature live probe

| Endpoint | Old bin :8006 | New bin clean data :8007/8008 |
|----------|---------------|-------------------------------|
| `/memory/graph/view` | 404 | **200** |
| `/memory/graph/entities` | 404 | **200** |
| `/code/graph/view` | 404 | **200** |
| `/code/stats` | 200 | **200** |
| `/panel/api/graph` | 404 | **500** (`no such table: panel_graphs` on fresh workspace store path) |

**Legacy production DB** fails startup with `schema_migrations has no column named name` — **fixed in source** (`ensure_migration_table` ALTER ADD COLUMN). Release rebuild blocked by file lock on `target/release/xavier.exe` while processes hold the file.

## Binary packaging

| Path | Status |
|------|--------|
| `target/release/xavier.exe` | Built (~54 MB, SHA256 `D512E235…`) — may be locked |
| `dist/xavier.exe` | Copied |
| `dist/xavier-ola-graph.exe` | Copied (install alias) |
| `C:\Users\belal\bin\xavier.exe` | **Locked** by watchdog/other processes — could not overwrite |
| `C:\Users\belal\bin\xavier-ola-graph.exe` | Install path for new binary (use this until bin unlock) |

### Install / switch production :8006

```powershell
# 1) Stop all xavier + any service/watchdog holding the file
Get-Process xavier | Stop-Process -Force
# 2) Copy
Copy-Item E:\proyectosSWAL\xavier\target\release\xavier.exe C:\Users\belal\bin\xavier.exe -Force
# 3) Start
$env:XAVIER_TOKEN = "<token>"
xavier http 8006 --mcp-port 8100
```

Or run side-by-side:

```powershell
C:\Users\belal\bin\xavier-ola-graph.exe http 8008 --mcp-port 0
```

## features.json (honest)

| Metric | Value |
|--------|-------|
| Overall (avg of feature progress_pct) | **99.5%** (was claimed 100% flat) |
| Features | **25** (+ `feat-graph-explorer`) |
| @100% complete | **23** |
| `feat-graph-explorer` | **92%** stable |
| `feat-mcp-server` | **95%** (tools/list protocol issue + unit fails) |

## Graph Explorer DoD scorecard

| Criterion | % | Evidence |
|-----------|---|----------|
| Roadmap CRUD code | 100 | #663 merged + unit tests |
| Memory KG API | 100 | #676+#675; live 200 on new bin |
| Code graph view | 100 | #674; live 200 |
| Multi-layer UI | 95 | #679; panel tests green; manual QA pending |
| EntityGraph durability | 95 | #677; unit/workspace tests |
| Legacy DB upgrade | 90 | fix landed + unit test; release reinstall blocked by lock |
| Docs | 70 | plan in memory; GRAPH_LAYERS.md residual |
| **Weighted feature** | **~92%** | |

## Plan registered in Xavier

Search: `ola graph explorer plan multi-layer`  
Memory id: `01KXSNW3EHZ0F5PQA6358NPKF7`
