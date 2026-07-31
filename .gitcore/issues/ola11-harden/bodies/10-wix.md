> Parent EPIC: {{EPIC}}

# [Ola 11 · 10] WiX installer: remove xavier-gui residual; FEATURE_STATUS sync

> Wave **Ola 11 — Harden Residuals**. Packaging honesty.

## Current State (MEDIBLE)
- `installer/xavier.wxs` still references `xavier-gui.exe` Component + shortcut Target
- FEATURE_STATUS already marks WiX Deprecated — keep consistent after WiX edit

## Desired State (DELTA)
- Remove `XavierGuiExe` component and shortcuts pointing at `xavier-gui.exe`
- Point primary shortcut at `xavier.exe` / documented CLI+panel layout OR remove GUI shortcut
- Update FEATURE_STATUS WiX/Panel lines if needed for truth (no marketing fluff)

## Web Research Required
1. Read `installer/xavier.wxs` fully
2. Read `installer/README.md` + Inno `setup.iss` for current supported layout (read-only)

## Exact Technical Context
- CRITICAL: ONLY `installer/xavier.wxs` and `docs/FEATURE_STATUS.md`
- Do not revive WiX as Stable

## Problem
Deprecated WiX still claims a binary that is not part of the product.

## Acceptance Criteria
- [ ] `grep -c xavier-gui installer/xavier.wxs` → 0
- [ ] FEATURE_STATUS does not claim xavier-gui ships
- [ ] Only listed files changed

## Files to Modify
| File | Change | Risk |
|------|--------|------|
| `installer/xavier.wxs` | Drop xavier-gui | MED |
| `docs/FEATURE_STATUS.md` | Sync wording | LOW |

## DO NOT touch
- `installer/setup.iss` unless required for consistency (prefer not)
- features.json

## Verification
```bash
grep -c xavier-gui installer/xavier.wxs || true
```

## Dependencies & Merge Order
- **Expected effort:** Small
