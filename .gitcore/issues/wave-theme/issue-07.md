# [Ola Theme.07] feat-top-status-bar-studio — Top Status Bar & Quick Theme Switcher

> Ola Theme — Visual Identity & System Redesign.
> Labels: `wave-theme`, `ola-theme`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` shows modified files BEFORE creating PR
- [ ] `git diff --stat HEAD` lists files (NOT empty)
- [ ] The PR MUST contain ≥1 file: verify with `git ls-files` before commit

## Current State (MEDIBLE)
- File: `panel-ui/src/components/ThemeToggle.tsx` (26 lines): only toggles binary `dark` vs `light`.
- File: `panel-ui/src/components/TopStatusBar.tsx`: uses legacy neon green styling.

## Desired State (DELTA)
- **Modify `panel-ui/src/components/ThemeToggle.tsx`**:
  - Transform into a responsive quick theme cycle/menu supporting:
    - Studio Dark (icon: Moon / Sparkle)
    - Bone White (icon: Sun / Feather)
    - Xavier Cyberpunk (icon: Terminal / Zap)
  - Add bordered button design (`icon-bordered`, `press-attenuation`).
- **Modify `panel-ui/src/components/TopStatusBar.tsx`**:
  - Update status pills, badge borders, and mesh/latency indicators to respect the active theme palette.
  - Borderless elevated bar with clean system typography.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "Three-way theme toggle icon cycle button React"
2. search: "Status bar responsive safe area desktop UI"
3. search: "Accessible popover theme switcher keyboard navigable"

## 🔬 Agent Session Prompt
"Before implementing:
1. Check `panel-ui/src/components/ThemeToggle.tsx` and see how it interacts with `useTheme()`.
2. Ensure clicking the button cycles `studio-dark` -> `studio-bone` -> `cyberpunk` smoothly."

## Existing Code Patterns (DEBES seguir estos)
- Lucide React icons with proper `aria-label` and `aria-expanded` attributes.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `grep -c "studio-bone" panel-ui/src/components/ThemeToggle.tsx` >= 1
- [ ] `grep -c "cyberpunk" panel-ui/src/components/ThemeToggle.tsx` >= 1
- [ ] `grep -c "press-attenuation" panel-ui/src/components/ThemeToggle.tsx` >= 1

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/src/components/ThemeToggle.tsx` | 26 lines | Multi-theme cycle button | LOW |
| `panel-ui/src/components/TopStatusBar.tsx` | Exists | Studio theme adaptation | MED |

## DO NOT touch (Anti-Regression)
- `panel-ui/src/components/NotificationsDropdown.tsx`
