# [Ola Theme.08] feat-input-area-chat-studio — Command Dock & Bordered Interactive Actions

> Ola Theme — Visual Identity & System Redesign.
> Labels: `wave-theme`, `ola-theme`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` shows modified files BEFORE creating PR
- [ ] `git diff --stat HEAD` lists files (NOT empty)
- [ ] The PR MUST contain ≥1 file: verify with `git ls-files` before commit

## Current State (MEDIBLE)
- File: `panel-ui/src/components/InputArea.tsx` (237 lines).
- Uses neon green focus rings and glow effects hardcoded (`#39ff14`).
- Action buttons have circular unbounded hover styles rather than bordered chips.

## Desired State (DELTA)
- **Modify `panel-ui/src/components/InputArea.tsx`**:
  - Restyle dock into a borderless, elevated studio capsule (`bg-[#151619]/90 dark:bg-[#151619]/90 backdrop-blur-xl border border-white/[0.06] rounded-2xl`).
  - Convert action buttons (`BrainCircuit`, `FolderPlus`, `Mic`, `Send`) to clean bordered controls with:
    - Subtle border (`border border-white/10 dark:border-white/10`).
    - Smooth hover attenuation (`hover:bg-white/[0.06]`).
    - Active press attenuation (`press-attenuation` / scale down).
  - Modernize audio recording visualizer: adapts to active theme color.
  - Full mobile responsiveness: dock width adapts automatically on narrow viewports without clipping action buttons.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "Modern AI prompt dock UI floating bar design"
2. search: "Responsive chat input bar mobile safe area"
3. search: "Outlined button micro-interaction active scale"
4. search: "Audio wave visualizer CSS animation clean design"

## 🔬 Agent Session Prompt
"Before implementing:
1. Review `InputArea.tsx` lines 120-230.
2. Replace hardcoded `#39ff14` classes with theme-aware accent and bordered icon wrappers."

## Existing Code Patterns (DEBES seguir estos)
- React.memo, useCallback, and Lucide React icons.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `grep -c "press-attenuation" panel-ui/src/components/InputArea.tsx` >= 2
- [ ] `grep -c "border-white/10" panel-ui/src/components/InputArea.tsx` >= 2
- [ ] Command input submits text cleanly on Enter

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/src/components/InputArea.tsx` | 237 lines | Studio aesthetic with bordered buttons | MED |

## DO NOT touch (Anti-Regression)
- `panel-ui/src/components/ChatHistory.tsx`
