# [WAVE-27.06] feat-telecom-agent-select — AgentResponderSelect dropdown for automated node cognitive responders

> Wave 27 — Autonomous Maintenance, Multi-Node Telecom UI & Hardening. Labels: `wave-27`, `ola27` (sin `jules` todavía)
> Merge order: 6/30 | Risk: MED | Effort: Medium 2-4h | Assigned: Cuenta A (Frontend)

---

## Current State (MEDIBLE)

- File: `panel-ui/src/components/Telecom/AgentResponderSelect.tsx` (0 lines - create new component)
- Feature: `feat-private-telecom-v2` at 0% in `.gitcore/features.json` (planned, REQ-060)
- Existing code: `src/telecom/` contains verified crypto, protocol, chat, store, gateway and tests in `main`.
- Need: Implement AgentResponderSelect allowing users to select which local AI agent (e.g. Legal, Sentinel, Research) answers incoming peer questions.

## Desired State (DELTA)

- **Target file**: `panel-ui/src/components/Telecom/AgentResponderSelect.tsx`
- Requirements:
  - Implement AgentResponderSelect allowing users to select which local AI agent (e.g. Legal, Sentinel, Research) answers incoming peer questions.
  - Strictly maintain 100% disjoint file isolation (touch ONLY `panel-ui/src/components/Telecom/AgentResponderSelect.tsx`).
  - Zero-warning clean compilation and comprehensive unit test coverage.
- Risk: MED — localized strictly to `panel-ui/src/components/Telecom/AgentResponderSelect.tsx`.

## 🌐 Web Research Required

**MANDATORY — 4 queries. El agente DEBE investigar antes de implementar.**
1. search: "Accessible custom select dropdown React 19"
2. search: "AI Agent picker component with capabilities"
3. search: "Switch toggle for automated bot response"
4. search: "Tailwind floating select menu"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read and understand existing patterns in the codebase:
   - For backend/Rust: inspect `src/telecom/mod.rs`, `src/telecom/chat.rs`, `src/storage/pragma.rs`.
   - For frontend/React: inspect `panel-ui/src/components/Mesh/MeshHubView.tsx`, `panel-ui/src/App.tsx`.
3. Check for API compatibility and avoid deprecated methods.
4. Ensure your implementation lives strictly in `panel-ui/src/components/Telecom/AgentResponderSelect.tsx` without modifying other files."

## Existing Code Patterns (MUST follow)

- Rust: `thiserror` for enums, `anyhow::Result` for fallible ops, `tokio` async, WAL pragmas.
- React: React 19, Tailwind CSS, Lucide icons, explicit `type="button"` on interactive buttons, strict ARIA accessibility.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `grep -cE "AgentResponderSelect|selectedAgent" panel-ui/src/components/Telecom/AgentResponderSelect.tsx >= 1`
- [ ] `pnpm --filter xavier-panel-ui run check`
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1 (PR contains files)

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/src/components/Telecom/AgentResponderSelect.tsx` | 0 lines - create new component | Implement feat-telecom-agent-select requirements | MED |

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

- [ ] `git status --porcelain` shows `panel-ui/src/components/Telecom/AgentResponderSelect.tsx` modified/untracked BEFORE opening PR.
- [ ] `git diff --stat HEAD` lists files (NOT empty).
- [ ] The PR MUST contain >= 1 file: verify with `git ls-files` before pushing.
- [ ] IF the task cannot be finished: DO NOT open PR — leave a comment detailing blockers on the issue.
