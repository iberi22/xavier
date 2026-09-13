# [Ola Theme.04] feat-settings-appearance-view — Appearance & Themes Settings View

> Ola Theme — Visual Identity & System Redesign.
> Labels: `wave-theme`, `ola-theme`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` shows modified files BEFORE creating PR
- [ ] `git diff --stat HEAD` lists files (NOT empty)
- [ ] The PR MUST contain ≥1 file: verify with `git ls-files` before commit

## Current State (MEDIBLE)
- `panel-ui/src/pages/Settings/` contains `Providers.tsx`, `Security.tsx`, `LeasesPage.tsx`, but NO `Appearance.tsx`.
- Users have no dedicated settings section to configure themes, system fonts, icon borders, or attenuation.

## Desired State (DELTA)
- **New file**: `panel-ui/src/pages/Settings/Appearance.tsx`:
  - Directly inspired by the settings window layout from the user screenshot:
    - Header: "Appearance" / "Aspecto" with subtitle "Customize theme, typography, and visual ergonomics".
    - Section: "Theme Preset" with 3 visual selector cards:
      1. **Studio Dark (Default / Predeterminado)**: Deep charcoal background, borderless surface, electric accent.
      2. **Bone White / Hueso Blanco**: Soft warm off-white parchment, high readability.
      3. **Xavier Cyberpunk (Secondary / Clásico)**: Historic green neon `#39ff14` terminal theme.
      4. **System (Sincronizado con SO)**.
    - Section: "Typography": Toggle between Native System Font and Monospace typography.
    - Section: "Micro-interactions & Borders":
      - Toggle for Outlined/Bordered Icons.
      - Toggle for Press & Hover Attenuation.
    - Section: "Interactive Preview": Live preview box showcasing buttons, pills, and inputs in real time.
- **Modify**: `panel-ui/src/pages/Settings/index.ts` to export `AppearancePage`.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "Settings appearance theme selector cards React Tailwind"
2. search: "Accessible theme picker UI patterns WAI-ARIA"
3. search: "Live theme preview component React"
4. search: "System font preference local storage persistence"

## 🔬 Agent Session Prompt
"Before implementing:
1. Examine `panel-ui/src/pages/Settings/Providers.tsx` and `panel-ui/src/pages/Settings/Security.tsx` to understand the layout and styling conventions.
2. Ensure the appearance page integrates seamlessly with `useTheme()`."

## Existing Code Patterns (DEBES seguir estos)
- `panel-ui/src/pages/Settings/Security.tsx` → Section card grouping with clean headers and action buttons.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `test -f panel-ui/src/pages/Settings/Appearance.tsx` returns 0
- [ ] `grep -c "studio-dark" panel-ui/src/pages/Settings/Appearance.tsx` >= 1
- [ ] `grep -c "studio-bone" panel-ui/src/pages/Settings/Appearance.tsx` >= 1
- [ ] `grep -c "cyberpunk" panel-ui/src/pages/Settings/Appearance.tsx` >= 1

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/src/pages/Settings/Appearance.tsx` | Non-existent | Create appearance settings view | MED |
| `panel-ui/src/pages/Settings/index.ts` | Exists or create | Export Appearance page | LOW |

## DO NOT touch (Anti-Regression)
- `panel-ui/src/pages/Settings/Security.tsx` — Security logic untouched
- `panel-ui/src/pages/Settings/Providers.tsx` — Provider logic untouched
