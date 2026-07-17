# [Ola 4 · 01] Headless chat: memory-fallback parity con el panel

> Parte de **Xavier 100% Local — Ola 4** (control plane + paridad de fallback).
> Contexto recuperado de Xavier: panel ya tiene fallback real (#605); headless devuelve 500 sin degradar.

## Web Research Required (Jules must search the web)

Before implementing, search the internet for:
1. **Axum IntoResponse custom JSON body on error** — search: `axum IntoResponse StatusCode Json 2024 pattern`
2. **OpenAI chat completion error response shape** — search: `openai api chat completion error response format` — keep response OpenAI-compatible if possible
3. Review how `ProxyErrorWrapper` works in this repo before changing error paths

## Exact Technical Context

- **File**: `src/cli/handlers/headless_api.rs` (~374 lines)
- **Function**: `headless_chat` at lines **129–158**
- **Current broken path** (lines 145–157):
```rust
match state.proxy_use_case.execute_secured(...).await {
    Ok(resp) => (StatusCode::OK, AxumJson(resp)).into_response(),
    Err(e) => ProxyErrorWrapper(e).into_response(), // <-- NO memory fallback
}
```
- **Gold pattern to copy** — `panel_process_chat_inner` in `src/cli/handlers/panel.rs` lines **166–203**:
  - On `Err(e)`: `state.usage_counters.record_memory_fallback();`
  - Then `state.memory.search(query, 5, None).await`
  - Return synthetic assistant content with prefix `[Modo memoria - LLM no disponible]`
- **State fields available**: `state.proxy_use_case`, `state.memory` (`MemoryQueryPort`), `state.usage_counters` (`Arc<UsageCounters>`), `state.secrets_engine`, `state.event_bus`
- **ChatCompletion type**: `xavier::domain::proxy::ChatCompletion` with `choices: Vec<ChatChoice>`, `ChatMessage { role, content }`, `Usage { prompt_tokens, completion_tokens, total_tokens }` at `src/domain/proxy/mod.rs`
- **User query extraction**: last user message from `req.messages` where `role == "user"`, or join contents

> CRITICAL: DO NOT touch `src/app/proxy_use_case.rs` (owned by other work). DO NOT touch `panel.rs`. DO NOT touch `xavier-core/`.

## Problem

`headless_chat` returns HTTP 500 when the LLM is down. The panel already degrades to memory search. CLI agents using `/v1/chat/completions` get no useful answer offline.

## Acceptance Criteria

- [ ] On `execute_secured` **Err**, call `state.usage_counters.record_memory_fallback()`
- [ ] Extract query string from the last user message in `req.messages`
- [ ] Call `state.memory.search(query, 5, None).await`
- [ ] If results non-empty: return **HTTP 200** with a `ChatCompletion` whose first choice content is:
  `format!("[Modo memoria - LLM no disponible]\n\n{}", context)` joined with `\n---\n`
- [ ] Set `model` field to `"memory-fallback"` on that response
- [ ] If memory also empty: return 200 with content `format!("[LLM no disponible: {}]", e)` OR keep 503 with JSON error — prefer 200 + message for agent UX
- [ ] Unit/integration test in `src/cli/handlers/headless_api.rs` `#[cfg(test)]` OR `tests/` that mocks/stubs path — at minimum a pure helper test for building the fallback completion from sample records
- [ ] `cargo check --workspace` 0 errors
- [ ] Diff must touch ONLY the files listed below (max 2 files). Empty commits forbidden.

## Files to Modify

| File | Change |
|---|---|
| `src/cli/handlers/headless_api.rs` | Replace Err arm of `headless_chat` (~155-156) with memory-fallback parity |
| `tests/e2e_chat_local.rs` (optional) | If easy, assert non-500; otherwise skip |

**DO NOT touch:** `proxy_use_case.rs`, `panel.rs`, `usage_counters.rs`, `xavier-core/`, `panel-ui/`, Cargo.toml

**NEVER create `.patch` / `.py` / `part1.rs` loose files in repo root.**
Edit `.rs` files directly. Run `cargo check --workspace` before PR.
If `git diff --stat` shows 0 files, the PR will be rejected.

## Verification

```bash
cargo check --workspace
cargo test -p xavier --lib headless 2>/dev/null || true
```

## Dependencies and Merge Order

- **Depends on:** nothing (UsageCounters already on main #607)
- **Can run in parallel with:** Ola 4 · 02, 03, 04 (different files)
- **Must merge before:** Ola 4 · 05 (E2E that asserts fallback)
