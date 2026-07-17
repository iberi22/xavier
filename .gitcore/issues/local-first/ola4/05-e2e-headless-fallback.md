# [Ola 4 · 05] Test E2E headless: fallback a memoria sin 500

> Parte de **Xavier 100% Local — Ola 4**.

## Web Research Required

1. **cargo integration test spawn binary** — search: `cargo test CARGO_BIN_EXE spawn server 2024`
2. Follow existing `tests/e2e_chat_local.rs` pattern (already on main)

## Exact Technical Context

- **Existing test**: `tests/e2e_chat_local.rs` (~89 lines) — spawns `xavier http`, hits chat with dead LLM URL
- **Current assertion weakness**: only checks status success; after Ola 4 · 01, assert:
  - HTTP 200
  - body model == `"memory-fallback"` OR content contains `"Modo memoria"` OR content not empty on fallback path
- **File only**: tests/ — do not change production code in this issue (assume 01 merged)

## Problem

No automated guarantee that headless never returns bare 500 when LLM is down after fallback work.

## Acceptance Criteria

- [ ] Update `tests/e2e_chat_local.rs` OR add `tests/e2e_headless_fallback.rs`
- [ ] Keep `#[ignore]` if test needs full binary + slow CI
- [ ] Document run: `cargo test -p xavier --test e2e_chat_local -- --ignored --nocapture`
- [ ] Assert: when `XAVIER_LOCAL_LLM_URL=http://127.0.0.1:1/v1`, chat does not return 500 after server up
- [ ] Prefer assert body contains memory fallback marker if 01 merged
- [ ] `cargo check --workspace` 0 errors
- [ ] Only touch tests/

## Files to Modify

| File | Change |
|---|---|
| `tests/e2e_chat_local.rs` | Strengthen assertions for fallback |

**DO NOT touch:** `src/`

## Dependencies and Merge Order

- **Depends on:** Ola 4 · 01 (headless fallback implementation)
- **Can run in parallel with:** 03, 04 after 01 merged
