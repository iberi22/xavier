# [Ola Theme.01] feat-theme-token-engine — Core Theme Provider & Multi-Theme Token Support

> Ola Theme — Visual Identity & System Redesign.
> Labels: `wave-theme`, `ola-theme`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` shows modified files BEFORE creating PR
- [ ] `git diff --stat HEAD` lists files (NOT empty)
- [ ] The PR MUST contain ≥1 file: verify with `git ls-files` before commit
- [ ] Verify that `git show HEAD --name-only` lists the same source files described in the PR

## Current State (MEDIBLE)
- File: `panel-ui/src/lib/theme/theme-provider.tsx` (75 lines, only supports `"dark" | "light" | "system"`, default is generic `"dark"`).
- Colors are hardcoded to `#050505` and `#ffffff` in `index.css`.
- No typed support for `studio-dark`, `studio-bone`, or `cyberpunk`.

## Desired State (DELTA)
- **New file**: `panel-ui/src/lib/theme/types.ts` defining:
  - `ThemeMode = "studio-dark" | "studio-bone" | "cyberpunk" | "system"`
  - Theme preference storage and configuration interfaces (font family, icon borders, attenuation).
- **New file**: `panel-ui/src/lib/theme/tokens.ts` exporting color palettes and theme constants for Studio Dark (`#0d0e10`), Bone White (`#f7f6f2`), and Cyberpunk (`#050505`).
- **Modify**: `panel-ui/src/lib/theme/theme-provider.tsx`:
  - Set default theme to `"studio-dark"`.
  - Apply `data-theme` attribute to `document.documentElement` alongside class names.
  - Dynamically synchronize `<meta name="theme-color">` to match active theme background.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "React 19 theme provider data-theme CSS variables pattern"
2. search: "Modern dark IDE UI color tokens obsidian charcoal palette"
3. search: "Off-white bone warm theme UI color system"
4. search: "Tailwind v4 dynamic theme variables data attribute"

## 🔬 Agent Session Prompt
"Before implementing, please:
1. Read `panel-ui/src/lib/theme/theme-provider.tsx` carefully to understand current context API.
2. Ensure backward compatibility so `useTheme()` returns `theme`, `resolvedTheme`, and `setTheme`.
3. Verify that `studio-dark` resolves as dark mode class and `studio-bone` as light mode class for standard Tailwind dark: variant compatibility."

## Existing Code Patterns (DEBES seguir estos)
- `panel-ui/src/lib/theme/theme-provider.tsx` → React Context + useState + localStorage pattern.
- Modern TypeScript strict types without `any`.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `grep -c "studio-dark" panel-ui/src/lib/theme/types.ts` >= 1
- [ ] `grep -c "studio-bone" panel-ui/src/lib/theme/types.ts` >= 1
- [ ] `grep -c "cyberpunk" panel-ui/src/lib/theme/types.ts` >= 1
- [ ] `grep -c 'defaultTheme = "studio-dark"' panel-ui/src/lib/theme/theme-provider.tsx` >= 1
- [ ] `pnpm --filter xavier-panel-ui run build` passes without type errors

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/src/lib/theme/types.ts` | Non-existent | Create with ThemeMode types | LOW |
| `panel-ui/src/lib/theme/tokens.ts` | Non-existent | Create with palette constants | LOW |
| `panel-ui/src/lib/theme/theme-provider.tsx` | 75 lines | Update with new theme logic | MED |

## DO NOT touch (Anti-Regression)
- `panel-ui/src/api/*` — Backend communication boundary
- `panel-ui/src/auth/*` — Auth boundaries

## Anti-Hallucination Guard ⚠️
1. **READ before write**: Read `panel-ui/src/lib/theme/theme-provider.tsx` before updating.
2. **Match existing patterns**: Keep Context Provider signature compatible with App.tsx.
3. **No extra dependencies**: Do not install additional npm packages; use pure TypeScript/React.
4. **Build verification**: Always run typecheck and build before marking complete.
