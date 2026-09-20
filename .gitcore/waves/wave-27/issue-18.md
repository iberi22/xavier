# [WAVE-27.18] feat-telecom-metrics — Prometheus metrics collector for telecom packets, latency & bandwidth

> Wave 27 — Autonomous Maintenance, Multi-Node Telecom UI & Hardening. Labels: `wave-27`, `ola27` (sin `jules` todavía)
> Merge order: 18/30 | Risk: MED | Effort: Medium 2-4h | Assigned: Cuenta B (Backend)

---

## Current State (MEDIBLE)

- File: `src/telecom/metrics.rs` (0 lines - create new module)
- Feature: `feat-private-telecom-v2` at 0% in `.gitcore/features.json` (planned, REQ-060)
- Existing code: `src/telecom/` contains verified crypto, protocol, chat, store, gateway and tests in `main`.
- Need: Implement Prometheus metrics tracking: `telecom_packets_sent_total`, `telecom_packets_received_total`, `telecom_bytes_total`, `telecom_handshake_duration_seconds`.

## Desired State (DELTA)

- **Target file**: `src/telecom/metrics.rs`
- Requirements:
  - Implement Prometheus metrics tracking: `telecom_packets_sent_total`, `telecom_packets_received_total`, `telecom_bytes_total`, `telecom_handshake_duration_seconds`.
  - Strictly maintain 100% disjoint file isolation (touch ONLY `src/telecom/metrics.rs`).
  - Zero-warning clean compilation and comprehensive unit test coverage.
- Risk: MED — localized strictly to `src/telecom/metrics.rs`.

## 🌐 Web Research Required

**MANDATORY — 4 queries. El agente DEBE investigar antes de implementar.**
1. search: "metrics crate prometheus exporter Rust 2026"
2. search: "Histogram timer pattern metrics Rust"
3. search: "Counter and Gauge thread-safe increments"
4. search: "Exposing metrics endpoint in axum router"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read and understand existing patterns in the codebase:
   - For backend/Rust: inspect `src/telecom/mod.rs`, `src/telecom/chat.rs`, `src/storage/pragma.rs`.
   - For frontend/React: inspect `panel-ui/src/components/Mesh/MeshHubView.tsx`, `panel-ui/src/App.tsx`.
3. Check for API compatibility and avoid deprecated methods.
4. Ensure your implementation lives strictly in `src/telecom/metrics.rs` without modifying other files."

## Existing Code Patterns (MUST follow)

- Rust: `thiserror` for enums, `anyhow::Result` for fallible ops, `tokio` async, WAL pragmas.
- React: React 19, Tailwind CSS, Lucide icons, explicit `type="button"` on interactive buttons, strict ARIA accessibility.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check -p xavier --all-targets 2>&1 | grep ^error | wc -l == 0`
- [ ] `grep -cE "TelecomMetrics|record_packet_sent" src/telecom/metrics.rs >= 1`
- [ ] `cargo test --lib telecom::metrics -- --nocapture 2>&1 | grep ok >= 1`
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1 (PR contains files)

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/telecom/metrics.rs` | 0 lines - create new module | Implement feat-telecom-metrics requirements | MED |

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

- [ ] `git status --porcelain` shows `src/telecom/metrics.rs` modified/untracked BEFORE opening PR.
- [ ] `git diff --stat HEAD` lists files (NOT empty).
- [ ] The PR MUST contain >= 1 file: verify with `git ls-files` before pushing.
- [ ] IF the task cannot be finished: DO NOT open PR — leave a comment detailing blockers on the issue.
