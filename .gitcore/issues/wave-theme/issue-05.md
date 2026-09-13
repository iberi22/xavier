# [Ola Theme.05] feat-config-modal-studio-theme — ConfigModal Appearance Tab & Borderless Styling

> Ola Theme — Visual Identity & System Redesign.
> Labels: `wave-theme`, `ola-theme`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` shows modified files BEFORE creating PR
- [ ] `git diff --stat HEAD` lists files (NOT empty)
- [ ] The PR MUST contain ≥1 file: verify with `git ls-files` before commit

## Current State (MEDIBLE)
- File: `panel-ui/src/components/ConfigModal.tsx` (1441 lines).
- Tabs list: `config`, `graph`, `bookmarks`, `providers`, `messaging`, `security`, `mesh`, `memory`, `agents`, `usage`, `plugins`.
- No `appearance` tab.
- Outer modal container has `glass` class with neon elements.

## Desired State (DELTA)
- **Modify `panel-ui/src/components/ConfigModal.tsx`**:
  - Add `"appearance"` to `MainTab` union type.
  - Add `TabButton` for Appearance with `Palette` icon in the navigation bar.
  - Render `<AppearancePage />` when `mainTab === "appearance"`.
  - Refine modal window container to borderless studio card look (`bg-[#141518]/95 dark:bg-[#141518]/95 backdrop-blur-xl border border-white/[0.06] rounded-[24px]`).
  - Add active pill styling matching the screenshot (`bg-[#26272b]` with subtle light text).

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "Modal dialog borderless glass elevation UI patterns"
2. search: "Tab navigation active indicator pill React Tailwind"
3. search: "Accessible tab list keyboard navigation arrow keys React"
4. search: "Responsive modal dialog max-height overflow container"

## 🔬 Agent Session Prompt
"Before implementing:
1. Carefully check where tabs are rendered in `panel-ui/src/components/ConfigModal.tsx` (around lines 360-440).
2. Wire `AppearancePage` cleanly without breaking existing graph and providers tabs."

## Existing Code Patterns (DEBES seguir estos)
- `ConfigModal.tsx` → `TabButton` component and `AnimatePresence` tab switches.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `grep -c 'mainTab === "appearance"' panel-ui/src/components/ConfigModal.tsx` >= 2
- [ ] `grep -c "Appearance" panel-ui/src/components/ConfigModal.tsx` >= 1
- [ ] Modal closes and opens smoothly without console errors

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/src/components/ConfigModal.tsx` | 1441 lines | Add Appearance tab & style refinements | MED |

## DO NOT touch (Anti-Regression)
- Graph rendering and force-directed algorithms in ConfigModal
