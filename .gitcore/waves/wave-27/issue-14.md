# [WAVE-27.14] feat-telecom-vitest-suite — Vitest test suite for TelecomHub & DirectChat component interactions

> Wave 27 — Autonomous Maintenance, Multi-Node Telecom UI & Hardening. Labels: `wave-27`, `ola27` (sin `jules` todavía)
> Merge order: 14/30 | Risk: MED | Effort: Medium 2-4h | Assigned: Cuenta A (Frontend)

---

## Current State (MEDIBLE)

- File: `panel-ui/tests/telecom-hub.spec.ts` (0 lines - create new test file)
- Feature: `feat-private-telecom-v2` at 0% in `.gitcore/features.json` (planned, REQ-060)
- Existing code: `src/telecom/` contains verified crypto, protocol, chat, store, gateway and tests in `main`.
- Need: Implement Vitest unit and integration tests verifying TelecomHubView tab transitions, message rendering, and mock WebSocket frame handling.

## Desired State (DELTA)

- **Target file**: `panel-ui/tests/telecom-hub.spec.ts`
- Requirements:
  - Implement Vitest unit and integration tests verifying TelecomHubView tab transitions, message rendering, and mock WebSocket frame handling.
  - Strictly maintain 100% disjoint file isolation (touch ONLY `panel-ui/tests/telecom-hub.spec.ts`).
  - Zero-warning clean compilation and comprehensive unit test coverage.
- Risk: MED — localized strictly to `panel-ui/tests/telecom-hub.spec.ts`.

## 🌐 Web Research Required

**MANDATORY — 4 queries. El agente DEBE investigar antes de implementar.**
1. search: "Vitest testing React component user-event"
2. search: "Mock WebSocket in Vitest test suite"
3. search: "Testing tab switching accessibility Vitest"
4. search: "Assert rendered text in React testing library"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read and understand existing patterns in the codebase:
   - For backend/Rust: inspect `src/telecom/mod.rs`, `src/telecom/chat.rs`, `src/storage/pragma.rs`.
   - For frontend/React: inspect `panel-ui/src/components/Mesh/MeshHubView.tsx`, `panel-ui/src/App.tsx`.
3. Check for API compatibility and avoid deprecated methods.
4. Ensure your implementation lives strictly in `panel-ui/tests/telecom-hub.spec.ts` without modifying other files."

## Existing Code Patterns (MUST follow)

- Rust: `thiserror` for enums, `anyhow::Result` for fallible ops, `tokio` async, WAL pragmas.
- React: React 19, Tailwind CSS, Lucide icons, explicit `type="button"` on interactive buttons, strict ARIA accessibility.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `grep -cE "describe|test\(" panel-ui/tests/telecom-hub.spec.ts >= 3`
- [ ] `pnpm --filter xavier-panel-ui run test panel-ui/tests/telecom-hub.spec.ts`
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1 (PR contains files)

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `panel-ui/tests/telecom-hub.spec.ts` | 0 lines - create new test file | Implement feat-telecom-vitest-suite requirements | MED |

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

- [ ] `git status --porcelain` shows `panel-ui/tests/telecom-hub.spec.ts` modified/untracked BEFORE opening PR.
- [ ] `git diff --stat HEAD` lists files (NOT empty).
- [ ] The PR MUST contain >= 1 file: verify with `git ls-files` before pushing.
- [ ] IF the task cannot be finished: DO NOT open PR — leave a comment detailing blockers on the issue.
