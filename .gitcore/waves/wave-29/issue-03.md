# [WAVE-29.03] feat-guardian-truth — env_status reads system scope; log_scan reads oldest-first

> Wave 29 — Observability honesty. Labels: `wave-29`, `ola29` (NO `jules` yet)
> Merge order: 3/10 | Risk: LOW | Effort: Small 1-2h

---

## Current State (MEDIBLE)

File: `src/self_manage/mod.rs` (~900 lines)

**Defect 1 — `check_service_status` queries the WRONG systemd scope (lines 769-786):**
```rust
pub fn check_service_status(service: &str) -> String {
    match Command::new("systemctl")
        .args(["is-active", service])
        .output()
    {
        Ok(output) => {
            let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if status.is_empty() { "inactive".to_string() } else { status }
        }
        Err(_) => "unknown (systemctl unavailable)".to_string(),
    }
}
```
Xavier is a **user** unit (`~/.config/systemd/user/xavier.service`). `systemctl is-active` with no
`--user` queries the *system* manager, where that unit does not exist.

Measured contradiction, same moment, same host:
- `systemctl --user is-active xavier.service` → `active`, `systemctl --user status` →
  `active (running) since Fri 2026-09-25 23:01:31`, Main PID 908606, up 10h45m.
- The `env_status` MCP tool reported `services: {xavier.service: "inactive",
  hermes.service: "inactive", openclaw.service: "inactive"}` — **all three wrong**.

A guardian that trusts this will conclude the service is down and can trigger an unnecessary
restart of a perfectly healthy process.

**Defect 2 — `get_sorted_log_files` sorts ASCENDING, i.e. OLDEST FIRST (lines 462-474):**
```rust
files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
```
Log filenames are `xavier.YYYY-MM-DD` and rotate daily at ~18:58, so ascending name order IS
ascending age order. `log_scan` (line 552) then iterates `for file_path in files` and honours a
persisted cursor (`start_reading` at line 570, `cursor.last_file`).

Measured: `~/.xavier/logs/` holds 15 rotated files, newest `xavier.2026-09-26` (1.5MB, modified
the same day). The tool returned a cursor of `{"last_file":"xavier.2026-09-05","last_line":5696}`
— **21 days stale** — and all 40 returned entries were from `2026-09-05`, all
`ERROR file is not a database`. Response carried `truncated: true` and `histogram: {"ERROR": 41}`,
presenting 21-day-old data as the current state. `max_entries` was exhausted inside the stale
file, so the active log was never reached.

**Defect 3 — no grouping or deduplication.** 41 byte-identical lines consumed the entire
`max_entries` budget, leaving no room for distinct events.

Existing tests in this file that must keep passing: `log_scan_tests` module (line 683, with a
call at 734), `env_status_tests` module (line 867, `test_env_status_snapshot` at 871), and the
`sys_health_*` tests at 308/337/354/370.

## Desired State (DELTA)

- **Line 770-786**:
  - Query the user manager: `systemctl --user is-active <service>`.
  - Make the scope explicit and configurable rather than hardcoded-in-silent: prefer a
    documented per-service scope, or try `--user` first and fall back to system scope, recording
    WHICH scope answered in the output so a reader can tell.
  - Never report `"inactive"` merely because stdout was empty. Distinguish
    "unit not found in this scope" from "unit is inactive" and from "systemctl unavailable".
    The current `if status.is_empty() { "inactive" }` conflates all three.
  - Add a short doc comment on `check_service_status` stating the scope it queries.
- **Line 472**: sort log files NEWEST FIRST for the default scan direction. The cursor must still
  work: when a cursor names a file that is no longer present, do not silently return stale data —
  either fall back to the newest file and report the reset, or return an explicit
  "cursor file rotated away" indication. Never return 21-day-old entries labelled as current.
- **Response shape** (`LogScanResult`, declared near line 421): report the real time range covered
  (earliest and latest entry timestamps) so a caller can tell whether it is looking at now or at
  history. Add fields if needed; keep existing fields backward compatible.
- **Grouping**: collapse repeated identical entries into one row carrying a `count`,
  `first_seen` and `last_seen`. Preserve `max_entries` as the budget for DISTINCT groups, not
  raw lines, so one repeated error cannot consume the whole response.
- **New tests** in `log_scan_tests` / `env_status_tests`:
  - `log_scan_prefers_newest_file_when_cursor_is_stale`
  - `log_scan_reports_time_range_of_returned_entries`
  - `log_scan_groups_repeated_identical_lines`
  - `check_service_status_distinguishes_missing_unit_from_inactive`
- Risk: LOW — these are read-only reporting paths. `scheduler/daemon.rs` (lines 208, 217) also
  calls `env_status` and `log_scan`; additive response fields are safe.

## 🌐 Web Research Required

**MANDATORY — 4 queries. Research BEFORE implementing.**
1. search: "systemctl --user vs system scope is-active exit status meaning"
2. search: "systemctl is-active exit code 4 unit not found inactive difference"
3. search: "Rust read_dir sort files newest first mtime lexicographic date filename"
4. search: "log aggregation deduplicate repeated lines group count first_seen last_seen"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read `src/self_manage/mod.rs` COMPLETELY (~900 lines). Focus on:
   - `check_service_status` (lines 769-786)
   - `get_sorted_log_files` (lines 462-474)
   - `log_scan` (line 552 onward), `LogScanArgs` (~390), `LogScanResult` (~421),
     `load_cursor` / cursor persistence, `parse_log_line` (line 477)
   - the `log_scan_tests` (683) and `env_status_tests` (867) modules
   - `resolve_logs_dir`
3. Execute in 3 micro-phases:
   - **Phase 1 — Types & Contracts**: add the time-range and grouping fields to
     `LogScanResult`; document the systemd scope decision.
   - **Phase 2 — Core Logic**: fix the scope in `check_service_status`, fix the sort direction,
     implement stale-cursor handling, implement grouping.
   - **Phase 3 — Tests & Delivery**: add the 4 named tests, run verification, open the PR.
4. **Blocker Recovery**: on compiler/test failure, search the web for the exact error signature,
   fix it, document it in the PR body. NEVER wait for feedback. NEVER delete an existing test."

## Existing Code Patterns (MUST follow)

- Argv execution via `std::process::Command` (not a shell string) — keep it that way.
- `#[cfg(test)] mod log_scan_tests` / `mod env_status_tests` already exist in this file; add
  tests inside them, do not create new test files.
- `Alert` struct and the alert thresholds already used by `collect_system_snapshot` are the
  model for any new reporting field.
- Comments in English, minimal density.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep -c "^error"` == 0
- [ ] `cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep -c "^error"` == 0
- [ ] `cargo test --package xavier --lib --features ci-safe self_manage -- --test-threads=1 2>&1 | grep "test result"` shows 0 failed
- [ ] `cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | grep "test result: FAILED"` produces NO output
- [ ] `grep -n 'args(\["is-active", service\])' src/self_manage/mod.rs` returns NO match
- [ ] `grep -n '"--user"' src/self_manage/mod.rs` >= 1 match
- [ ] `grep -n 'a.file_name().cmp(&b.file_name())' src/self_manage/mod.rs` returns NO match
- [ ] `grep -c "count\|first_seen\|last_seen" src/self_manage/mod.rs` >= 3
- [ ] `cargo fmt --all -- --check` produces no diff
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/self_manage/mod.rs` | ~900 lines; `check_service_status` 769, `get_sorted_log_files` 462, `log_scan` 552 | Fix scope, fix sort, stale-cursor handling, grouping, time range, 4 tests | LOW |

## DO NOT touch (Anti-Regression)

- `src/health/mod.rs` — island of WAVE-29.01.
- `src/server/mcp/tools_core.rs`, `src/server/mcp/types.rs`, `src/server/mcp/tests.rs` —
  islands of WAVE-29.02. `tools_core.rs` calls `self_manage::log_scan` and
  `self_manage::env_status`; that call site must keep compiling unchanged.
- `src/scheduler/daemon.rs` — it consumes both functions (lines 208, 217). Do NOT edit it;
  additive response fields are enough.
- `src/context/skill_dispatcher.rs`, `src/context/skill_registry.rs` — island of WAVE-29.04.
- `src/memory/**` — island of WAVE-29.09.
- `.gitcore/features.json`, `Cargo.toml`, `Cargo.lock`, `tests/**`, `.github/workflows/**`.

## Anti-Hallucination Guard ⚠️

1. **READ before write**: read the whole file. `log_scan` has a non-obvious cursor protocol and
   the `start_reading` logic at line 570 interacts with the loop.
2. **Do not assume the cursor format**: read `load_cursor` and the `LogCursor` struct before
   changing persistence. If you change the on-disk format, handle the old format gracefully.
3. **Do not shell out with a formatted string** — keep `Command::new("systemctl").args([...])`.
4. **Do not "fix" the embedding health** — different island, different file.
5. **`cargo check` gate**: 0 errors before commit.

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows `src/self_manage/mod.rs` modified BEFORE opening the PR
- [ ] `git diff --stat HEAD` is NOT empty
- [ ] `git show HEAD --name-only | grep -c "src/self_manage/mod.rs"` >= 1
- [ ] PR body shows the BEFORE/AFTER of `check_service_status` and of the sort comparator
- [ ] PR body explains the stale-cursor policy you chose and why
- [ ] PR body shows the new test names and their `test result` line
- [ ] If the work could not be completed: DO NOT open a PR — comment the blocker on this issue.

## Verification

```bash
cargo fmt --all -- --check
cargo check --package xavier --all-targets --features ci-safe 2>&1 | grep "^error" | wc -l   # expect 0
cargo clippy --package xavier --all-targets --features ci-safe -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --package xavier --lib --features ci-safe -- --test-threads=1 2>&1 | tail -20
```

## Dependencies & Merge Order

- **Depends on:** nothing. Merge order **3/10**.
- **Parallel with:** 29.01, 29.02, 29.04, 29.05, 29.06, 29.07, 29.08, 29.09, 29.10.
- **Expected effort:** Small 1-2h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix errors, do NOT commit broken code |
| Changing sort direction breaks an existing `log_scan` test | The test encodes the buggy behavior — update it and say so explicitly in the PR body |
| `systemctl --user` unavailable in the sandboxed CI runner | Make the scope injectable/mockable and unit-test the parsing, not the live call |
| Cursor format migration breaks existing cursors on disk | Support both formats; never delete a user's cursor file |
| `LogScanResult` change breaks `scheduler/daemon.rs` compile | That file is off-limits — keep every existing field and its type |
