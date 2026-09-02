# Xavier — Known Issues & Remediation Guide

> Last updated: 2026-09-01 — covers browser-compat wave5, config gaps, and SQLite WAL remediation.

## Known Bugs (active)

| # | Bug / Broken Path | Impact | Workaround | Fix |
|---|---|---|---|---|
| 1 | `panel-ui` without Tauri guard → `invoke`/`listen` crash `TypeError/Cannot read transformCallback` | 🔴 Critical browser | Use `__TAURI_INTERNALS__` guard + dynamic import (wave5) | Fixed 4af11709 |
| 2 | `get_xavier_token` → `""` → 401 loop `/v1/memories`/`/notifications` | 🔴 | Hook `useApiToken` reading `VITE_XAVIER_API_TOKEN` | Fixed |
| 3 | `GET /v1/config` 404, real is `/v1/config/providers` | 🟡 hasConfig never true | Fallback `hasConfig=true` (Ollama local) | Fixed |
| 4 | `GET /panel/api/config` 404 | 🟡 | Use `/v1/config/providers` | Fixed |
| 5 | `AuthProvider` refresh 400 clears `token` → loses `API_TOKEN` | 🟡 logout loop | Preserve `API_TOKEN` in catch | Fixed |
| 6 | `InputArea` `scan_project_folder` no File API fallback | 🟡 button dead | `webkitdirectory` hidden input | Fixed |
| 7 | `vite __APP_VERSION__` hardcode `0.6.1-beta` vs `0.0.1` | 🟢 | Read `Cargo.toml` via `fs` | Fixed b4a0fd2c |
| 8 | `pnpm.overrides` deprecated warning | 🟢 | Migrate to `pnpm-workspace.yaml` | Fixed |
| 9 | `XAVIER_TOKEN=foo # comment` inline → token includes `# comment` | 🟡 silent auth fail | Put comment on own line | Docs fix |
| 10 | `.env` vs `config/xavier.config.json` canon confusion | 🟡 | Settings loader is source of truth | Docs fix |
| 11 | `panel-ui/build` vs `panel-ui/dist` drift (Dockerfile vs vite vs assets.rs) | 🟡 Docker serves empty | Align to `dist` | Fixed |
| 12 | `XAVIER_WORKSPACE_DIR=.` vs `workspace_id=default` | 🟢 | Documented | Docs |
| 13 | `FEATURE_STATUS.md` says `1.0.0` when canon `0.0.1` | 🟢 drift | Align to 0.0.1 | Fixed below |
| 14 | `docs/api/README.md` Spanish when README EN | 🟢 i18n | Translate | Pending wave6 |
| 15 | `docker-compose` without `XAVIER_DEV_MODE` | 🟡 dev needs token | Add `XAVIER_DEV_MODE` | Fixed |
| 16 | SQLite WAL size exceeds threshold (`wal_size_bytes` > 50MB / 55175072 bytes) causing `database.status` to report `unhealthy` | 🟡 Database degraded / health check flapping | Execute manual checkpoint with `PRAGMA wal_checkpoint(TRUNCATE); VACUUM;` or run `scripts/checkpoint.sh` | Fixed in `src/storage/pragma.rs` (WAVE 7.03) |

## WAL & Health Remediation

SQLite Write-Ahead Logging (WAL) mode enables high concurrency and fast write operations by appending modifications to a sidecar file (`.sqlite3-wal`). However, without periodic checkpointing, high-throughput write streams (such as rapid vector embeddings, memory records, and graph updates) can cause the WAL file to grow indefinitely—reaching sizes such as 55MB (`55175072` bytes) or larger.

When `wal_size_bytes` exceeds 50MB (52,428,800 bytes) or when database integrity checks flap, Xavier's observability probe marks the system database health as degraded or `unhealthy`.

### Diagnostic & Health Probe Overview

Operators can check the database health metrics by querying the system health REST endpoint:

```bash
curl -s http://127.0.0.1:8006/health | python3 -m json.tool
```

An unhealthy database response due to un-checkpointed WAL frames will present output similar to:

```json
{
  "status": "unhealthy",
  "components": {
    "database": {
      "status": "degraded",
      "wal_size_bytes": 55175072,
      "integrity_ok": true,
      "fragmentation_percent": 12.4
    }
  }
}
```

### Symptom, Cause, and Fix Summary

| Symptom | Cause | Fix |
|---|---|---|
| `curl /health` reports `database.status: degraded` or `unhealthy` with `wal_size_bytes: 55175072` (or > 50MB) | WAL log file is un-checkpointed and accumulation of uncommitted/un-truncated WAL frames exceeds safety threshold (50MB). | Run SQLite checkpoint & vacuum: `sqlite3 ~/.local/share/xavier/memory-store.sqlite3 "PRAGMA wal_checkpoint(TRUNCATE); VACUUM;"` and restart Xavier. |
| Intermittent `database.status` flapping between `ok` and `degraded` under heavy write concurrency | High concurrent transaction load preventing default SQLite passive checkpointing from truncating the WAL file. | Enforce startup and periodic WAL threshold checks using `src/storage/pragma.rs` / `src/storage/migrations.rs` and configure `PRAGMA journal_size_limit`. |

### Manual Remediation Steps

If an operator encounters a bloated WAL file or `unhealthy` database status, execute the following manual remediation procedures:

1. **Locate the Active SQLite Database File:**
   Xavier stores its primary memory and vector SQLite databases under the user data directory or a path specified by `XAVIER_MEMORY_SQLITE_PATH`:
   - Default Linux/macOS path: `~/.local/share/xavier/memory-store.sqlite3`
   - Data directory paths: `data/code_graph.db`, `data/vec-store.sqlite3`

2. **Run Manual Checkpoint and Vacuum:**
   Execute a synchronous `TRUNCATE` checkpoint to flush all WAL pages into the main database file and reset the WAL file size to 0 bytes, followed by a `VACUUM` to defragment storage:

   ```bash
   sqlite3 ~/.local/share/xavier/memory-store.sqlite3 "PRAGMA wal_checkpoint(TRUNCATE); VACUUM;"
   ```

   Alternatively, for standard Xavier deployments, execute the automated helper script:

   ```bash
   bash scripts/checkpoint.sh
   ```

3. **Restart the Xavier Engine:**
   Restart the Xavier service to apply clean connections and verify health recovery:

   ```bash
   curl -s http://127.0.0.1:8006/health | grep wal_size_bytes
   ```

### Code-Level Mitigation & Pragmas (WAVE-7.03)

In WAVE 7.03, automated WAL maintenance was integrated directly into Xavier's database core (`src/storage/pragma.rs` and `src/storage/migrations.rs`). The system automatically enforces:

- **Centralized PRAGMA Configuration (`src/storage/pragma.rs`):**
  Sets `PRAGMA journal_mode = WAL;`, `PRAGMA synchronous = NORMAL;`, `PRAGMA cache_size = -8000;`, and `PRAGMA temp_store = MEMORY;`.
- **Automatic WAL Size Threshold Checks (`maybe_wal_checkpoint`):**
  Checks WAL size against `WAL_CHECKPOINT_THRESHOLD_BYTES` (50MB). When surpassed, Xavier automatically issues `PRAGMA wal_checkpoint(TRUNCATE);` to prune the WAL frame index.
- **Journal Size Limit (`PRAGMA journal_size_limit`):**
  Caps maximum WAL journal disk retention to prevent unbounded disk usage under high transaction volumes.

## Broken Config Paths

- `XAVIER_TOKEN` inline comments: `XAVIER_TOKEN=foo # comment` becomes literal token with `# comment`. Fix: put `# comment` on its own line. Documented in `docs/reference/ENV_VARS.md`.
- `XAVIER_PANEL_UI_DIR` not in README `Environment Variables` table. Priority: 1) `XAVIER_PANEL_UI_DIR` env, 2) `<exe_dir>/panel-ui/build`, 3) `<exe_dir>/panel-ui`, 4) `<cwd>/panel-ui/build`, 5) `CARGO_MANIFEST_DIR/panel-ui/build` (see `src/server/panel/assets.rs`).
- `XAVIER_CODE_GRAPH_DB_PATH`, `XAVIER_MEMORY_*_PATH` in compose but not in README — add to ENV_VARS table.

See also: `docs/reference/ENV_VARS.md`, `README.md` Known Issues.
