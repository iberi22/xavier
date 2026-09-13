# [Ola Theme.02] feat-theme-css-tokens-attenuation — CSS Tokens, Micro-Interactions, and System Fonts

> Ola Theme — Visual Identity & System Redesign.
> Labels: `wave-theme`, `ola-theme`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` shows modified files BEFORE creating PR
- [ ] `git diff --stat HEAD` lists files (NOT empty)
- [ ] The PR MUST contain ≥1 file: verify with `git ls-files` before commit

## Current State (MEDIBLE)
- File: `panel-ui/src/index.css` (139 lines).
- Uses Google Web Fonts (`Inter`, `JetBrains Mono`).
- Only defines `:root` and `.dark` variables with `#050505` and `#ffffff`.
- Glow and active tabs hardcode neon green `#39ff14`.

## Desired State (DELTA)
- **Modify `panel-ui/src/index.css`**:
  - Add system font stack: `--font-system-sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif` and system monospace `--font-system-mono`.
  - Add theme scopes:
    - `:root[data-theme="studio-dark"]` and `.studio-dark`: `--background: #0d0e10; --surface: #141518; --surface-card: #1b1c20; --foreground: #f3f3f5; --muted: #8f919a; --border-subtle: rgba(255, 255, 255, 0.06); --accent: #3b82f6;`
    - `:root[data-theme="studio-bone"]` and `.studio-bone`: `--background: #f7f6f2; --surface: #ebe8e1; --surface-card: #ffffff; --foreground: #1c1c1f; --muted: #73716c; --border-subtle: rgba(0, 0, 0, 0.08); --accent: #2563eb;`
    - `:root[data-theme="cyberpunk"]` and `.cyberpunk`: `--background: #050505; --surface: #0a0a0a; --surface-card: #141414; --foreground: #ffffff; --muted: #888888; --border-subtle: rgba(57, 255, 20, 0.2); --accent: #39ff14;`
  - Add micro-interaction classes:
    - `.press-attenuation`: `active:scale-[0.98] active:opacity-75 transition-all duration-100 ease-out`
    - `.hover-attenuation`: `hover:bg-white/[0.04] transition-colors duration-150 ease-out`
    - `.surface-borderless`: card/panel elevated purely by tonal contrast without thick borders.
    - `.icon-bordered`: crisp border, rounded corners, centered icon.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "Apple Human Interface Guidelines system font CSS fallback"
2. search: "CSS active scale press micro-interaction smooth easing"
3. search: "Borderless UI card elevation techniques flat design"
4. search: "Bone white warm palette hex codes UI design"

## 🔬 Agent Session Prompt
"Before implementing:
1. Examine existing scrollbar, neon glow, and glass classes in `panel-ui/src/index.css`.
2. Ensure existing `.neon-glow` and `.active-tab` remain functional when in cyberpunk mode.
3. Add utility classes that work smoothly with Tailwind v4."

## Existing Code Patterns (DEBES seguir estos)
- `panel-ui/src/index.css` → Tailwind v4 `@theme` directive, CSS variables, keyframe animations.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `grep -c "studio-dark" panel-ui/src/index.css` >= 1
- [ ] `grep -c "studio-bone" panel-ui/src/index.css` >= 1
- [ ] `grep -c "press-attenuation" panel-ui/src/index.css` >= 1
- [ ] `grep -c "hover-attenuation" panel-ui/src/index.css` >= 1
- [ ] `grep -c "icon-bordered" panel-ui/src/index.css` >= 1

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/src/index.css` | 139 lines | Add tokens, utilities, and font rules | MED |

## DO NOT touch (Anti-Regression)
- `panel-ui/vite.config.ts` — Build config
- `panel-ui/src/App.tsx` — Separate island

## Anti-Hallucination Guard ⚠️
1. **Preserve existing styles**: Do not delete existing glass or neon styles; enhance them.
2. **Tailwind v4 compatibility**: Use standard CSS properties and `@theme` compatibility.
3. **No breaking color variables**: Keep `--background` and `--foreground` as baseline fallbacks.
