# [Ola Theme.10] feat-theme-verification-test-suite — Automated Theme Verification & Test Suite

> Ola Theme — Visual Identity & System Redesign.
> Labels: `wave-theme`, `ola-theme`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` shows modified files BEFORE creating PR
- [ ] `git diff --stat HEAD` lists files (NOT empty)
- [ ] The PR MUST contain ≥1 file: verify with `git ls-files` before commit

## Current State (MEDIBLE)
- Vitest suite has 30 test files, but NO dedicated tests for multi-theme resolution (`studio-dark`, `studio-bone`, `cyberpunk`).
- No unit tests validating Appearance settings behavior.

## Desired State (DELTA)
- **New file**: `panel-ui/tests/theme-system.test.tsx`:
  - Test default theme initialization is `studio-dark`.
  - Test switching theme changes `localStorage` and `document.documentElement` attributes.
  - Test switching to `studio-bone` updates attributes and light mode class.
  - Test switching to `cyberpunk` activates neon legacy tokens.
- **New file**: `panel-ui/tests/appearance-settings.test.tsx`:
  - Test Appearance settings tab rendering in ConfigModal.
  - Test theme card clicks switch active theme.
  - Test micro-interaction toggles (icon borders, attenuation).

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "Testing-library react testing localStorage theme provider"
2. search: "Vitest document.documentElement classList tests"
3. search: "Vitest mock matchMedia system theme"
4. search: "React 19 testing-library renderHook theme context"

## 🔬 Agent Session Prompt
"Before implementing:
1. Examine `panel-ui/tests/DataNodeDashboard.test.tsx` for vitest and testing-library setup patterns.
2. Write clean, non-flaky unit tests."

## Existing Code Patterns (DEBES seguir estos)
- `@testing-library/react` and `vitest` assertions.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `pnpm --filter xavier-panel-ui test tests/theme-system.test.tsx` passes
- [ ] `pnpm --filter xavier-panel-ui test tests/appearance-settings.test.tsx` passes
- [ ] `pnpm --filter xavier-panel-ui run build` completes successfully

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/tests/theme-system.test.tsx` | Non-existent | Unit tests for theme provider | LOW |
| `panel-ui/tests/appearance-settings.test.tsx` | Non-existent | Tests for Appearance page | LOW |

## DO NOT touch (Anti-Regression)
- Existing tests in `panel-ui/tests/`
