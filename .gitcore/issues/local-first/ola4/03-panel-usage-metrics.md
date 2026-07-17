# [Ola 4 · 03] Panel UI: widget de métricas de uso local vs cloud

> Parte de **Xavier 100% Local — Ola 4**. Surfaces live UsageCounters from API (#578 already on main).

## Web Research Required

1. **React fetch polling pattern 2024** — search: `react useEffect fetch poll interval cleanup 2024`
2. **OpenAPI-style usage dashboard UX** — search: `LLM usage dashboard tokens per provider UI pattern`

## Exact Technical Context

- **API already live**: `GET /v1/account/usage` returns:
```json
{
  "status": "ok",
  "requests_used": 0,
  "total_tokens": 0,
  "total_errors": 0,
  "total_cost_usd": 0.0,
  "memory_fallback_hits": 0,
  "fallback_chain_hops": 0,
  "by_provider": { "local": { "requests": 0, "tokens": 0, "errors": 0, "cost_usd": 0.0 } }
}
```
  Handler: `src/cli/handlers/usage.rs` ~16–80
- **UI patterns to follow**:
  - `panel-ui/src/components/QuotaTable.tsx` (table styling)
  - `panel-ui/src/components/OperationModeBadge.tsx` / `TopStatusBar.tsx` for placement
  - `panel-ui/src/components/ProviderSelector.tsx` for dark glass styles
- Auth header: same as other panel fetches (`X-Xavier-Token` or session cookie — match existing `App.tsx` / API client pattern)

## Problem

UsageCounters exist server-side but operators cannot see local vs cloud token traffic or memory-fallback hits in the panel.

## Acceptance Criteria

- [ ] New component `panel-ui/src/components/UsageMetricsPanel.tsx`
- [ ] Fetches `/v1/account/usage` every 5–10s (or on mount + refresh button)
- [ ] Displays: total requests, tokens, errors, memory_fallback_hits, fallback_chain_hops
- [ ] Table of `by_provider` (provider, requests, tokens, errors, cost)
- [ ] Highlight row where provider name contains `local` or `ollama` in green accent `#39ff14`
- [ ] Wire component into existing layout (TopStatusBar area OR ConfigModal OR App.tsx dashboard section) — minimal invasive mount
- [ ] TypeScript builds: `cd panel-ui && npx vite build` (or project’s existing panel build command)
- [ ] DO NOT change Rust backend
- [ ] Empty PR forbidden; max ~5 files under `panel-ui/`

## Files to Modify

| File | Change |
|---|---|
| `panel-ui/src/components/UsageMetricsPanel.tsx` (NEW) | Widget |
| `panel-ui/src/App.tsx` or `TopStatusBar.tsx` or `ConfigModal.tsx` | Mount widget |

**DO NOT touch:** `src/**/*.rs`, `xavier-core/`, Cargo.toml

**NEVER create `.patch` files.**

## Verification

```bash
cd panel-ui && npx vite build
```

## Dependencies and Merge Order

- **Depends on:** #578 already merged (API ready on main)
- **Can run in parallel with:** 01, 02
- **Independent of:** 04 (different component files — if both edit App.tsx, merge 03 first then 04)
