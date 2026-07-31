> Parent EPIC: {{EPIC}}

# [Ola 11 · 06] Align embedding_provider_mode to local-first

> Wave **Ola 11 — Harden Residuals**. Host `--lib` failure.

## Current State (MEDIBLE)
- Fail: `settings::tests::test_load_config_json` expects `workspace.embedding_provider_mode == "local"`
- `config/xavier.config.json` has `"embedding_provider_mode": "bring_your_own"`
- `src/settings/defaults.rs` default also `"bring_your_own"`

## Desired State (DELTA)
- Config + defaults match local-first product contract used by the test (`"local"`)
- `test_load_config_json` passes without weakening assertions

## Web Research Required
1. Read test `test_load_config_json` in `src/settings/mod.rs` (read-only)
2. Confirm docs/local-first language in FEATURE_STATUS / AGENTS (read-only)

## Exact Technical Context
- CRITICAL: ONLY `config/xavier.config.json` and `src/settings/defaults.rs`
- Do NOT edit `src/settings/mod.rs` unless absolutely required for compile (prefer not)

## Problem
Shipped config contradicts local-first defaults claimed by tests.

## Acceptance Criteria
- [ ] `grep embedding_provider_mode config/xavier.config.json` shows `local`
- [ ] defaults.rs default is `local`
- [ ] `cargo test -p xavier --lib settings::tests::test_load_config_json` passes
- [ ] Only listed files changed

## Files to Modify
| File | Change | Risk |
|------|--------|------|
| `config/xavier.config.json` | embedding_provider_mode → local | MED |
| `src/settings/defaults.rs` | default → local | MED |

## DO NOT touch
- `src/settings/mod.rs` (unless forced), panel-ui, features.json

## Verification
```bash
CARGO_TARGET_DIR=/tmp/rt cargo test -p xavier --lib settings::tests::test_load_config_json -- --nocapture
```

## Dependencies & Merge Order
- **Expected effort:** Small
