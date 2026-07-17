# [Ola 4 · 04] Panel UI: hot-swap de modelos Ollama (list / pull / set active)

> Parte de **Xavier 100% Local — Ola 4**. UI for issue Ola 4 · 02 API.

## Web Research Required

1. **Ollama model names UX** — search: `Ollama model picker UI pull progress pattern`
2. **Accessible select + async button React** — search: `react accessible select async loading button 2024`

## Exact Technical Context

- **Depends on API from Ola 4 · 02** (merge that PR first or implement against the route contract below):
  - `GET /v1/ollama/models`
  - `POST /v1/ollama/pull` body `{ "name": "..." }`
  - `POST /v1/ollama/active` body `{ "model": "...", "kind": "llm" }`
  - `GET /v1/ollama/active`
- UI style: same as `ProviderSelector.tsx` / dark glass `#050505`
- Place near `ProviderSelector` or inside Config / local-first settings

## Problem

No UI to switch Ollama chat model without restarting Xavier.

## Acceptance Criteria

- [ ] New `panel-ui/src/components/OllamaModelManager.tsx`
- [ ] On mount: load models list + active model
- [ ] Dropdown/select of available models; button "Set active" calls POST active
- [ ] Input + "Pull" button for new model name; show loading/error states
- [ ] On Ollama down (503): show friendly message "Ollama no responde en :11434"
- [ ] Mount in App/Config near ProviderSelector
- [ ] `npx vite build` passes
- [ ] DO NOT modify Rust except if API missing — then fail with clear comment linking issue 02 (prefer wait for 02)
- [ ] Max ~5 panel-ui files; no root patches

## Files to Modify

| File | Change |
|---|---|
| `panel-ui/src/components/OllamaModelManager.tsx` (NEW) | Full UI |
| `panel-ui/src/App.tsx` or Config component | Mount |

**DO NOT touch:** `src/**/*.rs` (unless API issue 02 not merged and you only add the API in a SEPARATE commit — preferred: only UI)

## Verification

```bash
cd panel-ui && npx vite build
```

## Dependencies and Merge Order

- **Depends on:** Ola 4 · 02 (API) — **merge 02 first**
- **Can run in parallel with:** 01, 03 only until App.tsx conflict risk
- **If both 03 and 04 touch App.tsx:** merge 03 first, then rebase 04
