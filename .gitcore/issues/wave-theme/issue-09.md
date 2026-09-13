# [Ola Theme.09] feat-responsive-pwa-desktop-adapt — Responsiveness, Desktop Windowing, and PWA Enhancements

> Ola Theme — Visual Identity & System Redesign.
> Labels: `wave-theme`, `ola-theme`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` shows modified files BEFORE creating PR
- [ ] `git diff --stat HEAD` lists files (NOT empty)
- [ ] The PR MUST contain ≥1 file: verify with `git ls-files` before commit

## Current State (MEDIBLE)
- `panel-ui/index.html` has hardcoded `<meta name="theme-color" content="#0284c7" />`.
- `panel-ui/manifest.webmanifest` theme color does not match Studio Dark `#0d0e10`.
- Mobile screens on PWA or small viewports experience modal clipping on small heights.

## Desired State (DELTA)
- **Modify `panel-ui/index.html`**:
  - Update default meta theme-color to `#0d0e10`.
  - Add `viewport-fit=cover` to meta viewport for iPhone dynamic island and Android gesture bar safe-area insets.
- **Modify `panel-ui/build/manifest.webmanifest` and public/manifest if applicable**:
  - Update `theme_color` and `background_color` to `#0d0e10`.
- **New file**: `panel-ui/src/components/ResponsiveDrawer.tsx`:
  - Drawer / bottom-sheet container for mobile screen sizes (<640px) to gracefully render settings/navigation without overflow.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "PWA viewport-fit cover safe area inset env CSS"
2. search: "Dynamic theme-color meta tag sync dark light mode"
3. search: "Tauri window titlebar overlay CSS safe area desktop"
4. search: "Mobile bottom sheet dialog accessible pattern React"

## 🔬 Agent Session Prompt
"Before implementing:
1. Inspect `panel-ui/index.html` and manifest.
2. Verify safe area padding (`pb-[env(safe-area-inset-bottom)]`) across mobile views."

## Existing Code Patterns (DEBES seguir estos)
- HTML5 standards and responsive mobile PWA standards.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `grep -c "0d0e10" panel-ui/index.html` >= 1
- [ ] `grep -c "viewport-fit=cover" panel-ui/index.html` >= 1
- [ ] `test -f panel-ui/src/components/ResponsiveDrawer.tsx` returns 0

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/index.html` | 26 lines | Safe area and Studio Dark theme-color | LOW |
| `panel-ui/src/components/ResponsiveDrawer.tsx` | Non-existent | Mobile drawer component | LOW |

## DO NOT touch (Anti-Regression)
- `panel-ui/vite.config.ts`
