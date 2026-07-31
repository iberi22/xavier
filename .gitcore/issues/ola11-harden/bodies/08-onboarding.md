> Parent EPIC: {{EPIC}}

# [Ola 11 · 08] Onboarding auth step works without mandatory Tauri invoke

> Wave **Ola 11 — Harden Residuals**. Panel dual-runtime honesty.

## Current State (MEDIBLE)
- `panel-ui/src/components/Onboarding/OnboardingFlow.tsx` uses `invoke("save_initial_config")` (Tauri-only)
- `AuthStep.tsx` exists in onboarding
- Browser `/panel` path cannot complete onboarding if invoke fails

## Desired State (DELTA)
- Detect non-Tauri (web) and use HTTP auth/register or skip-save with localStorage flag
- Keep Tauri path working
- Auth step remains in flow without dead ends

## Web Research Required
1. search: `detect tauri webview vs browser frontend 2025`
2. Read AuthStep + OnboardingFlow fully
3. Check existing panel API auth helpers in panel-ui (read-only other files)

## Exact Technical Context
- CRITICAL: ONLY the two listed TSX files
- Do not redesign whole onboarding visual system
- Prefer existing auth API client if already imported elsewhere — copy minimal fetch if needed inside these files only

## Problem
Web panel onboarding is coupled to Tauri; fails or dead-ends in browser.

## Acceptance Criteria
- [ ] Non-Tauri path does not call `invoke` unconditionally on complete
- [ ] Auth step usable in web (register/login via HTTP or clear skip)
- [ ] `cd panel-ui && npx vite build` succeeds
- [ ] Only listed files changed

## Files to Modify
| File | Change | Risk |
|------|--------|------|
| `panel-ui/src/components/Onboarding/OnboardingFlow.tsx` | Web vs Tauri complete | MED |
| `panel-ui/src/components/Onboarding/AuthStep.tsx` | HTTP auth path | MED |

## DO NOT touch
- Other Onboarding steps, Rust backend, features.json

## Verification
```bash
cd panel-ui && npx vite build
```

## Dependencies & Merge Order
- **Expected effort:** Medium
