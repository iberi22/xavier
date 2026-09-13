# [Ola Theme.06] feat-app-shell-studio-modernization — Main Canvas Shell & Adaptive Background

> Ola Theme — Visual Identity & System Redesign.
> Labels: `wave-theme`, `ola-theme`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` shows modified files BEFORE creating PR
- [ ] `git diff --stat HEAD` lists files (NOT empty)
- [ ] The PR MUST contain ≥1 file: verify with `git ls-files` before commit

## Current State (MEDIBLE)
- File: `panel-ui/src/App.tsx` (643 lines):
  - Line 553: `bg-slate-50 dark:bg-[#050505] text-slate-900 dark:text-white`
  - Line 638: `<ThemeProvider defaultTheme="dark" storageKey="vite-ui-theme">`
- File: `panel-ui/src/components/ParticleBackground.tsx` (172 lines) draws neon green particles.

## Desired State (DELTA)
- **Modify `panel-ui/src/App.tsx`**:
  - Update `ThemeProvider` default theme to `"studio-dark"`.
  - Update main background container classes to use theme CSS variables:
    `bg-[var(--background,#0d0e10)] text-[var(--foreground,#f3f3f5)] font-system-sans`.
  - Ensure modal overlay has a subtle, premium backdrop blur matching the image.
- **Modify `panel-ui/src/components/ParticleBackground.tsx`**:
  - Make particle colors adapt dynamically to the theme:
    - In `studio-dark`: Soft translucent white/gray particles (`rgba(255, 255, 255, 0.08)`).
    - In `studio-bone`: Warm subtle taupe particles (`rgba(30, 30, 30, 0.05)`).
    - In `cyberpunk`: Vibrant green particles (`rgba(57, 255, 20, 0.15)`).

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "Canvas 2D getComputedStyle CSS variable performance"
2. search: "Modern dark background UI design tokens best practices"
3. search: "Seamless theme transition CSS opacity canvas"

## 🔬 Agent Session Prompt
"Before implementing:
1. Review `App.tsx` line 550-640.
2. In `ParticleBackground.tsx`, observe how particles are drawn and hook up theme awareness cleanly."

## Existing Code Patterns (DEBES seguir estos)
- React.memo and requestAnimationFrame loop in `ParticleBackground.tsx`.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `grep -c 'defaultTheme="studio-dark"' panel-ui/src/App.tsx` >= 1
- [ ] `grep -c "useTheme" panel-ui/src/components/ParticleBackground.tsx` >= 1
- [ ] No regression in chat history or draggable widgets

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/src/App.tsx` | 643 lines | Default theme to studio-dark & canvas classes | MED |
| `panel-ui/src/components/ParticleBackground.tsx` | 172 lines | Theme-aware particle rendering | LOW |

## DO NOT touch (Anti-Regression)
- `panel-ui/src/components/ChatHistory.tsx`
- `panel-ui/src/components/DraggableWidget.tsx`
