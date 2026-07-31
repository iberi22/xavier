# Xavier Feature Status

Current product label: `0.6.1-beta`

This matrix is the operational truth for the repository as of v0.6.1-beta stabilization.

## Release Status

| Surface | Current status | Notes |
|---|---|---|
|| HTTP health/readiness | Stable | `/health` and `/readiness` work correctly. |
|| HTTP memory add/search/stats | Stable | Authenticated memory write and search work. CLI, HTTP, and MCP contracts aligned. |
|| Canonical runtime config | Stable | `config/xavier.config.json` is the canonical source of non-secret config. `.env` is reserved for secrets and credentials. 25+ settings migrated. All env var reads funnel through settings loader. |
|| CLI add/search/stats | Stable | Commands act as HTTP clients using `XAVIER_URL` (primary) or `XAVIER_HOST`+`XAVIER_PORT` (fallback). No hardcoded port output. Contract aligned with server. |
|| MCP stdio | Stable | 12 tools (15 with aliases): create_memory, search_memory, get_memory, stats, list_projects, get_project_context, sync_gitcore, save_fragment/search_fragments/get_recent_fragments/memoryfragment_get/memoryfragment_delete. Security scanning applied uniformly. Protocol 2025-03-26. |
|| Panel shell/API | Stable (with assets) | The CLI server exposes `/panel` (frontend shell). Building frontend assets requires Node.js 22+ and pnpm (automatically compiled and copied by `install.ps1` when Node/pnpm is available, or manually built via `cd panel-ui && pnpm install && pnpm run build` and placed at `panel-ui/build` next to the binary). Panel API routes work independently of frontend assets. |
|| Tauri Desktop Installer | Stable (Canonical) | Tauri is the canonical desktop packaging solution on Windows. Configured in `panel-ui/src-tauri`. |
|| Inno Setup Portable | Stable | Inno Setup packaging generates portable installer setups (`XavierSetup.exe`) bundling the built CLI and Panel UI assets. |
|| WiX Toolset Installer | Deprecated | WiX MSI installer setup (`installer/xavier.wxs`) is deprecated in favor of Tauri and Inno Setup. |
|| Windows CI Workflow | Disabled / Staged | The `integration-windows.yml` workflow (when present) manages automatic Windows build pipelines and packaging. |
|| Release smoke scripts | Stable | Shell and PowerShell smoke scripts test identical contract (`/health`, `/readiness`, auth gate, memory add/search, usage). `/build` and panel checks are optional. Pre-commit hooks verify auth. |
|| Workspace/storage isolation | Stable | Canonical env vars documented. `XAVIER_WORKSPACE_DIR` / `XAVIER_DATA_DIR` fully isolate runtime. `seed_workspace()` documented as intentional. Tests prove temp workspace isolation. |
|| Public docs consistency | Stable | README, MCP_CONTRACT.md, ARCHITECTURE.md, FEATURE_STATUS.md, smoke scripts all aligned with v1.0 surface. |


## XTSP (Token-Saving Protocol) — Ola 9

| Feature | Status | Notes |
|---|--------|-------|
| Snippet search via `mode` parameter | Stable | `POST /v1/memories/search` accepts `mode=ids|snippet|full`. Hard cap 8KB. Page-in support. |
| Semantic dedup on `memory_add` | Stable | Configurable per-workspace via `dedup` policy. Default: disabled (opt-in). |
| Soft-delete prune with audit | Stable | Tombstone-based pruning. `memory_prune` tool removes soft-deleted records with audit trail. |
| MCP Streamable HTTP (port 8100) | Stable | Native MCP server with Streamable HTTP transport. Tools: `create_memory`, `search_memory`, `memory_prune`. |
| Clavis key masking | Stable | API keys masked in logs. Auto-rotation support. |
| GPU sidecar (`xavier gpud`) | Experimental | 3 endpoints (health, detect, serve). No mesh, no discovery, no plugin system. |
| Provider router local URIs | Stable | Local model URIs resolved correctly in provider router. |

## XTSP Integration Tests

All 7 XTSP end-to-end tests pass:
- `xtsp_fat_search`, `xtsp_page_in`, `xtsp_dedup`, `xtsp_full_flow`, `xtsp_persist`, `xtsp_prune`, `xtsp_token_savings`

See `tests/xtsp/` for the complete integration test suite.
## What Was Verified

### Confirmed working

- `xavier http`
- `POST /memory/add`
- `POST /memory/search`
- `GET /memory/stats`
- `GET /health`
- auth gate on protected routes
- `xavier mcp`
- MCP `tools/list`
- MCP `tools/call` for `add` and `search`

### Confirmed not 1.0-ready

- CLI commands do not behave like a purely embedded local memory tool; they depend on the HTTP server.
- `scripts/release-smoke.ps1` expects `/build`, but the tested server path returned `404`.
- Panel routes require built frontend assets and are not yet a complete release-ready surface.
- The current repo still contains insecure or stale references in scripts and docs that a public 1.0 release should not carry.
- The codebase still has many direct `std::env::var(...)` reads that need to finish migrating behind the canonical JSON config contract.

## Definition Of `1.0`

Xavier should only be labeled `1.0` when all of the following are true:

- one canonical server contract exists for CLI, HTTP, MCP, panel, and smoke scripts
- token and port behavior are documented and consistent
- release smoke passes without manual patching
- workspace and storage isolation are reproducible
- panel build and route expectations are either stable or clearly scoped out
- public dataset export emits reproducible read-only context with documented schema versions
- public docs describe the real product surface, not the aspirational one
- remaining `dev-token` and insecure-default references are removed from production-facing surfaces
