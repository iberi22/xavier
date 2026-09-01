# Xavier — Known Issues & Broken Config Paths

> Last updated: 2026-09-01 — covers browser-compat wave5 + config gaps for distribution.

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

## Broken Config Paths

- `XAVIER_TOKEN` inline comments: `XAVIER_TOKEN=foo # comment` becomes literal token with `# comment`. Fix: put `# comment` on its own line. Documented in `docs/reference/ENV_VARS.md`.
- `XAVIER_PANEL_UI_DIR` not in README `Environment Variables` table. Priority: 1) `XAVIER_PANEL_UI_DIR` env, 2) `<exe_dir>/panel-ui/build`, 3) `<exe_dir>/panel-ui`, 4) `<cwd>/panel-ui/build`, 5) `CARGO_MANIFEST_DIR/panel-ui/build` (see `src/server/panel/assets.rs`).
- `XAVIER_CODE_GRAPH_DB_PATH`, `XAVIER_MEMORY_*_PATH` in compose but not in README — add to ENV_VARS table.

See also: `docs/reference/ENV_VARS.md`, `README.md` Known Issues.
