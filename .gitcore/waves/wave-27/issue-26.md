# [WAVE-27.26] feat-telecom-stress-concurrency — Concurrency stress test simulating 50 simultaneous node message floods

> Wave 27 — Autonomous Maintenance, Multi-Node Telecom UI & Hardening. Labels: `wave-27`, `ola27` (sin `jules` todavía)
> Merge order: 26/30 | Risk: MED | Effort: Medium 2-4h | Assigned: Cuenta B (Backend)

---

## Current State (MEDIBLE)

- File: `tests/telecom_concurrency_stress_test.rs` (0 lines - create new test file)
- Feature: `feat-private-telecom-v2` at 0% in `.gitcore/features.json` (planned, REQ-060)
- Existing code: `src/telecom/` contains verified crypto, protocol, chat, store, gateway and tests in `main`.
- Need: Implement integration test spawning 50 concurrent tokio tasks sending 100 messages each through telecom gateway, asserting 0 dropped messages and clean WAL commits.

## Desired State (DELTA)

- **Target file**: `tests/telecom_concurrency_stress_test.rs`
- Requirements:
  - Implement integration test spawning 50 concurrent tokio tasks sending 100 messages each through telecom gateway, asserting 0 dropped messages and clean WAL commits.
  - Strictly maintain 100% disjoint file isolation (touch ONLY `tests/telecom_concurrency_stress_test.rs`).
  - Zero-warning clean compilation and comprehensive unit test coverage.
- Risk: MED — localized strictly to `tests/telecom_concurrency_stress_test.rs`.

## 🌐 Web Research Required

**MANDATORY — 4 queries. El agente DEBE investigar antes de implementar.**
1. search: "Tokio concurrent task barrier test Rust"
2. search: "SQLite under high concurrency WAL mode test"
3. search: "Assert message ordering in concurrent queue"
4. search: "Memory leak detection in stress test"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read and understand existing patterns in the codebase:
   - For backend/Rust: inspect `src/telecom/mod.rs`, `src/telecom/chat.rs`, `src/storage/pragma.rs`.
   - For frontend/React: inspect `panel-ui/src/components/Mesh/MeshHubView.tsx`, `panel-ui/src/App.tsx`.
3. Check for API compatibility and avoid deprecated methods.
4. Ensure your implementation lives strictly in `tests/telecom_concurrency_stress_test.rs` without modifying other files."

## Existing Code Patterns (MUST follow)

- Rust: `thiserror` for enums, `anyhow::Result` for fallible ops, `tokio` async, WAL pragmas.
- React: React 19, Tailwind CSS, Lucide icons, explicit `type="button"` on interactive buttons, strict ARIA accessibility.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check --test telecom_concurrency_stress_test 2>&1 | grep ^error | wc -l == 0`
- [ ] `grep -cE "#\[tokio::test\]" tests/telecom_concurrency_stress_test.rs >= 1`
- [ ] `cargo test --test telecom_concurrency_stress_test -- --nocapture 2>&1 | grep ok >= 1`
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1 (PR contains files)

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `tests/telecom_concurrency_stress_test.rs` | 0 lines - create new test file | Implement feat-telecom-stress-concurrency requirements | MED |

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

- [ ] `git status --porcelain` shows `tests/telecom_concurrency_stress_test.rs` modified/untracked BEFORE opening PR.
- [ ] `git diff --stat HEAD` lists files (NOT empty).
- [ ] The PR MUST contain >= 1 file: verify with `git ls-files` before pushing.
- [ ] IF the task cannot be finished: DO NOT open PR — leave a comment detailing blockers on the issue.
