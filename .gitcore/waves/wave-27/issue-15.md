# [WAVE-27.15] feat-telecom-a11y-audit — Accessibility & keyboard navigation spec for all Telecom components

> Wave 27 — Autonomous Maintenance, Multi-Node Telecom UI & Hardening. Labels: `wave-27`, `ola27` (sin `jules` todavía)
> Merge order: 15/30 | Risk: MED | Effort: Medium 2-4h | Assigned: Cuenta A (Frontend)

---

## Current State (MEDIBLE)

- File: `panel-ui/tests/telecom-a11y.spec.ts` (0 lines - create new test file)
- Feature: `feat-private-telecom-v2` at 0% in `.gitcore/features.json` (planned, REQ-060)
- Existing code: `src/telecom/` contains verified crypto, protocol, chat, store, gateway and tests in `main`.
- Need: Implement comprehensive axe-core / a11y assertions verifying keyboard navigation, tab order, focus ring presence, and ARIA attributes across all Telecom UI elements.

## Desired State (DELTA)

- **Target file**: `panel-ui/tests/telecom-a11y.spec.ts`
- Requirements:
  - Implement comprehensive axe-core / a11y assertions verifying keyboard navigation, tab order, focus ring presence, and ARIA attributes across all Telecom UI elements.
  - Strictly maintain 100% disjoint file isolation (touch ONLY `panel-ui/tests/telecom-a11y.spec.ts`).
  - Zero-warning clean compilation and comprehensive unit test coverage.
- Risk: MED — localized strictly to `panel-ui/tests/telecom-a11y.spec.ts`.

## 🌐 Web Research Required

**MANDATORY — 4 queries. El agente DEBE investigar antes de implementar.**
1. search: "axe-core testing library vitest accessibility"
2. search: "Keyboard navigation tab enter space test vitest"
3. search: "Check aria attributes and role compliance test"
4. search: "WCAG 2.1 AA automated compliance assertions"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read and understand existing patterns in the codebase:
   - For backend/Rust: inspect `src/telecom/mod.rs`, `src/telecom/chat.rs`, `src/storage/pragma.rs`.
   - For frontend/React: inspect `panel-ui/src/components/Mesh/MeshHubView.tsx`, `panel-ui/src/App.tsx`.
3. Check for API compatibility and avoid deprecated methods.
4. Ensure your implementation lives strictly in `panel-ui/tests/telecom-a11y.spec.ts` without modifying other files."

## Existing Code Patterns (MUST follow)

- Rust: `thiserror` for enums, `anyhow::Result` for fallible ops, `tokio` async, WAL pragmas.
- React: React 19, Tailwind CSS, Lucide icons, explicit `type="button"` on interactive buttons, strict ARIA accessibility.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `grep -cE "axe|checkA11y|accessibility" panel-ui/tests/telecom-a11y.spec.ts >= 2`
- [ ] `pnpm --filter xavier-panel-ui run test panel-ui/tests/telecom-a11y.spec.ts`
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1 (PR contains files)

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/tests/telecom-a11y.spec.ts` | 0 lines - create new test file | Implement feat-telecom-a11y-audit requirements | MED |

## DO NOT touch (Anti-Regression)

- `crates/xavier-core-logic` — core vector primitives must remain untouched.
- `.gitcore/features.json` — reconciled at wave end only by the orchestrator.
- Any other file in `src/telecom/` or `panel-ui/src/` assigned to parallel wave tasks (maintain disjoint file islands).

## Anti-Hallucination Guard ⚠️

1. **READ before write**: Inspect existing codebase patterns before writing any code.
2. **No inventar imports**: Only use crates/packages already in `Cargo.toml` or `package.json`.
3. **Check gate**: Must produce 0 errors before committing.
4. **No tocar `.gitcore/features.json`**: Reconciled only after all wave PRs are merged.

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows `panel-ui/tests/telecom-a11y.spec.ts` modified/untracked BEFORE opening PR.
- [ ] `git diff --stat HEAD` lists files (NOT empty).
- [ ] The PR MUST contain >= 1 file: verify with `git ls-files` before pushing.
- [ ] IF the task cannot be finished: DO NOT open PR — leave a comment detailing blockers on the issue.
