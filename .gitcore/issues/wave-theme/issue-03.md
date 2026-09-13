# [Ola Theme.03] feat-bordered-icons-ui-primitives — Bordered Icon System and Micro-Interaction UI Primitives

> Ola Theme — Visual Identity & System Redesign.
> Labels: `wave-theme`, `ola-theme`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` shows modified files BEFORE creating PR
- [ ] `git diff --stat HEAD` lists files (NOT empty)
- [ ] The PR MUST contain ≥1 file: verify with `git ls-files` before commit

## Current State (MEDIBLE)
- Directory `panel-ui/src/components/ui/` does not have standardized bordered icon wrappers or segmented pill selectors.
- Buttons across `InputArea` and `ConfigModal` use inline arbitrary Tailwind classes.

## Desired State (DELTA)
- **New file**: `panel-ui/src/components/ui/BorderedIcon.tsx`:
  - Outlined icon container with rounded corners (`rounded-lg` / `rounded-md`), subtle border (`border-white/10` or light equivalent), hover highlight, and press scale attenuation.
- **New file**: `panel-ui/src/components/ui/SegmentedControl.tsx`:
  - Segmented pill selector directly matching the screenshot's `Queue | Send Immediately` element with smooth active indicator and keyboard navigation.
- **New file**: `panel-ui/src/components/ui/ThemedButton.tsx`:
  - Button with bordered icon support, variant ("primary", "secondary", "ghost", "subtle"), and built-in press attenuation.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "Accessible segmented pill control React typescript"
2. search: "Micro-interaction active scale tap feedback CSS"
3. search: "Bordered icon container design token system"
4. search: "WAI-ARIA radiogroup segmented button accessibility"

## 🔬 Agent Session Prompt
"Before implementing:
1. Inspect the uploaded screenshot: notice the pills like `[Queue  Send Immediately]`, dropdowns `[Custom v]`, and the bordered icons.
2. Build flexible, accessible UI components that adapt to both Studio Dark and Bone White."

## Existing Code Patterns (DEBES seguir estos)
- React 19 functional components with TypeScript props and WAI-ARIA roles.
- Lucide React icon rendering.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `test -f panel-ui/src/components/ui/BorderedIcon.tsx` returns 0
- [ ] `test -f panel-ui/src/components/ui/SegmentedControl.tsx` returns 0
- [ ] `test -f panel-ui/src/components/ui/ThemedButton.tsx` returns 0
- [ ] `grep -c "press-attenuation" panel-ui/src/components/ui/BorderedIcon.tsx` >= 1

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/src/components/ui/BorderedIcon.tsx` | Non-existent | Create component | LOW |
| `panel-ui/src/components/ui/SegmentedControl.tsx` | Non-existent | Create component | LOW |
| `panel-ui/src/components/ui/ThemedButton.tsx` | Non-existent | Create component | LOW |

## DO NOT touch (Anti-Regression)
- Existing components in `panel-ui/src/components/*` (until integration issues)

## Anti-Hallucination Guard ⚠️
1. **Self-contained components**: Do not import uninstalled external libraries.
2. **Accessible markup**: Use `aria-pressed`, `aria-label`, and proper role attributes.
