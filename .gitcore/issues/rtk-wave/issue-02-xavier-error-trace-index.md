# [RTK Integration 02] feat-rtk-kernel-proxy — Index Failed Shell Command Traces into Memory

> Wave: `rtk-wave` · Area: `src/kernel/` · Protocol: GitCore 3.8.0
> Labels: `rtk-wave`, `kernel` (DO NOT attach `jules` until Phase 4 dispatch)

---

## Current State (MEDIBLE)
- Feature: `feat-rtk-kernel-proxy` at 100% in `.gitcore/features.json`.
- File: `src/kernel/runner.rs` executes shell commands via `execute_proxy_command`, applies condensation filters, and records token usage in `TRACKER`.
- Problem: When a command fails (`exit_code != 0`), the failure trace is returned to the caller but is not persisted in Xavier memory for future agent recall.

## Desired State (DELTA)
- **Specific Addition**:
  1. In `src/kernel/runner.rs`, if `exit_code != 0` and an optional workspace/memory reference is available, format the error snippet and failure summary into an automatic memory document:
     - `path`: `terminal/failures/{timestamp}_{hash}`
     - `kind`: `failure_trace`
     - `content`: condensed failure snippet
     - `metadata`: `{"command": cmd_line, "exit_code": exit_code}`
  2. Provide a helper function `index_command_failure(...)` so agents can query previous errors before re-attempting identical broken commands.
- **File Target**: `src/kernel/runner.rs`, `tests/kernel_proxy_test.rs`
- **Target Base Branch**: `wave/lean-modular-xavier`

## Web Research Required
1. search: "xavier create_memory rust internal api"
2. search: "agent error memory recall best practices"

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `cargo check --lib` — 0 errors
- [ ] `cargo test --test kernel_proxy_test` — all tests pass
- [ ] `grep -rn "index_command_failure" src/kernel/` >= 1 match

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/kernel/runner.rs` | 119 lines | Add failure trace memory formatting helper | LOW |
| `tests/kernel_proxy_test.rs` | ~85 lines | Add unit test verifying error trace formatting | LOW |

## DO NOT touch
- `panel-ui/` — assigned to Issue 01
- `src/kernel/filters.rs` — already stable

## Anti-Hallucination Guard
1. READ before write: inspect `src/kernel/runner.rs` and `src/memory/` schema.
2. Follow Rust 2021 idiomatic patterns with zero unsafe code.

## Merge Order
- **Merge order within wave:** 2
- **Expected effort:** Small (<30m)
- **Parallel with:** Issue 01 (disjoint file island)
