# [Ola 4 · 02] API Ollama models: list / pull / set-active (hot-swap backend)

> Parte de **Xavier 100% Local — Ola 4**. Backend for model hot-swap without process restart.

## Web Research Required (Jules must search the web)

1. **Ollama REST API tags/pull** — search: `Ollama API /api/tags /api/pull documentation 2024`
2. Official docs: https://github.com/ollama/ollama/blob/main/docs/api.md
3. **reqwest POST stream pull** — search: `reqwest post ollama pull json stream 2024` — pull may stream JSON lines; for MVP wait until final status or use non-stream if available

## Exact Technical Context

- Local base URL constant: `DEFAULT_LOCAL_BASE_URL = "http://localhost:11434/v1"` in `src/agents/provider/local.rs:13`
- Native Ollama API is at `http://localhost:11434` (strip `/v1`):
  - `GET /api/tags` → `{ "models": [ { "name": "llama3.2:latest", ... } ] }`
  - `POST /api/pull` body `{ "name": "llama3.2", "stream": false }`
- Active chat model env: `XAVIER_LOCAL_LLM_MODEL` (read in `src/agents/provider/config.rs` ~126–151)
- Active embedding env: `XAVIER_EMBEDDING_MODEL` (if present)
- Handler module pattern: see `src/cli/handlers/doctor.rs` and `usage.rs` for Axum handlers + `json_response`
- Register module in `src/cli/handlers/mod.rs` (add `pub mod ollama_models;`)
- Mount routes in `src/cli/server.rs` near other `/v1/` routes (~line 560–630). Prefer under auth-protected router if other `/v1/providers` are protected — mirror `/v1/providers` / headless routes.

Suggested routes:
```
GET  /v1/ollama/models          → list local models from Ollama
POST /v1/ollama/pull            → body { "name": "model:tag" }
POST /v1/ollama/active          → body { "model": "name", "kind": "llm"|"embedding" }
GET  /v1/ollama/active          → current env values
```

For `set-active`: update process env with `std::env::set_var("XAVIER_LOCAL_LLM_MODEL", model)` (and embedding if kind=embedding). Document that new ProxyUseCase config reads env on next request via `ModelProviderConfig::for_provider` / `from_env`.

> CRITICAL: Create ONE new file `src/cli/handlers/ollama_models.rs`. Minimal edits to `mod.rs` + `server.rs` only for export + routes.
> DO NOT rewrite `local.rs` or `config.rs` wholesale. DO NOT touch `proxy_use_case.rs`. DO NOT touch panel-ui (separate issue).
> DO NOT touch `xavier-core/`.

## Problem

Operators cannot list/pull/switch Ollama models without restarting Xavier or editing env files manually. Blocks 100% local-first UX.

## Acceptance Criteria

- [ ] New module `src/cli/handlers/ollama_models.rs` with handlers
- [ ] `list_models` uses `reqwest` GET to `{base}/api/tags` where base = `XAVIER_LOCAL_LLM_URL` stripped of trailing `/v1` or default `http://localhost:11434`
- [ ] `pull_model` POST `/api/pull` with `stream: false` (or consume stream until done)
- [ ] `set_active` sets `XAVIER_LOCAL_LLM_MODEL` or embedding env; returns JSON `{ "ok": true, "model": "...", "kind": "..." }`
- [ ] `get_active` returns current model envs
- [ ] On Ollama unreachable: return 503 JSON `{ "error": "ollama unreachable: ..." }` — do not panic
- [ ] Unit tests with `mockito` or `wiremock` if already in Cargo.toml; else pure URL-helper tests + `#[ignore]` integration test
- [ ] `cargo check --workspace` 0 errors
- [ ] Diff max ~4 files; empty PR forbidden

## Files to Modify

| File | Change |
|---|---|
| `src/cli/handlers/ollama_models.rs` (NEW) | Handlers list/pull/active |
| `src/cli/handlers/mod.rs` | `pub mod ollama_models;` |
| `src/cli/server.rs` | Register 4 routes only |

**DO NOT touch:** `panel-ui/`, `proxy_use_case.rs`, `config.rs` (except if reading helpers already public), `xavier-core/`, root patches

**NEVER create `.patch` / `.py` loose files.**

## Verification

```bash
cargo check --workspace
# Manual: ollama serve; curl localhost:8006/v1/ollama/models -H "X-Xavier-Token: $TOKEN"
```

## Dependencies and Merge Order

- **Depends on:** nothing
- **Can run in parallel with:** 01, 03
- **Must merge before:** 04 (panel hot-swap UI calls these endpoints)
