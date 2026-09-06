# [RTK Integration 01] feat-plugin-system — 1-Click RTK Kernel Proxy Installation UI in Panel

> Wave: `rtk-wave` · Area: `panel-ui` · Protocol: GitCore 3.8.0
> Labels: `rtk-wave`, `panel-ui` (DO NOT attach `jules` until Phase 4 dispatch)

---

## Current State (MEDIBLE)
- Feature: `feat-plugin-system` and `feat-rtk-kernel-proxy` stable in `.gitcore/features.json`.
- File: `panel-ui/src/components/ConfigModal.tsx` (~1421 lines) contains configuration tabs for providers, security, messaging, etc., but lacks a dedicated UI manager for dynamic plugins.
- Backend: Endpoints `GET /plugins` and `POST /plugins/install` are active via `src/cli/handlers/plugins.rs`.

## Desired State (DELTA)
- **Specific Addition**:
  1. Add a `"plugins"` tab in `panel-ui/src/components/ConfigModal.tsx` navigation.
  2. Implement a dedicated subcomponent `panel-ui/src/components/PluginsManager.tsx` that fetches available plugins from `GET /plugins` and renders an "Install with 1-Click" button.
  3. When clicking install on `rtk-kernel`, post to `POST /plugins/install` with payload `{"name": "rtk-kernel"}` and show an active badge "Active / Enabled".
- **File Target**: `panel-ui/src/components/PluginsManager.tsx`, `panel-ui/src/components/ConfigModal.tsx`
- **Target Base Branch**: `wave/lean-modular-xavier`

## Web Research Required
1. search: "react lucide-react plugin puzzle icons"
2. search: "swal panel-ui component conventions motion/react"

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `cd panel-ui && npm run build` — 0 errors, production build succeeds
- [ ] `grep -rn "PluginsManager" panel-ui/src/components/` >= 1 match
- [ ] `grep -rn "rtk-kernel" panel-ui/src/components/` >= 1 match

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/src/components/PluginsManager.tsx` | Non-existent | Create component with 1-click install card for plugins | LOW |
| `panel-ui/src/components/ConfigModal.tsx` | 1421 lines | Add "plugins" tab and mount `PluginsManager` | LOW |

## DO NOT touch
- `src/kernel/` — already verified and frozen for this micro-task
- `.gitcore/features.json` — reconciled at wave end

## Anti-Hallucination Guard
1. Inspect existing imports and styling patterns in `panel-ui/src/components/ConfigModal.tsx`.
2. Use Tailwind CSS classes consistent with panel-ui dark mode design system.

## Merge Order
- **Merge order within wave:** 1
- **Expected effort:** Small (<30m)
- **Parallel with:** Issue 02 (disjoint file island)
