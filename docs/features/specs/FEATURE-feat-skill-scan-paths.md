# FEATURE: Skill Registry Scans Canonical Store

**Status:** `planned` | **Score:** — | **Last Tested:** —

## Overview
`SkillRegistry` (`src/context/skill_registry.rs`, 345 lines) indexes skill files for dispatch, but `with_defaults` (lines 68-74) only scans `workspace_root/skills` + `workspace_root/.agents/skills`. The canonical 349-skill store at `~/.hermes/skills` is never indexed, so live `GET /skills` returns `count: 0` and every dispatch resolves `_none` (verified 2026-09-20). This feature adds the canonical path with cycle-safe traversal and Hermes `skills.disabled` respect.

## Architecture & Design
- `$HOME` resolved from environment at runtime (12-factor; no personal paths in code).
- `WalkDir` traversal with a visited `(device, inode)` set: symlink *files* are followed (system convention), symlink *cycles* terminate. Precedent: karpathy-principles 2-cycle (fixed 2026-09-20).
- Disabled-skill filtering reads Hermes `config.yaml` best-effort; unreadable/missing config → no filtering (fail-open), never an error.
- Incremental reindex by sha256 (`index_skill_file`, lines 114-156) is preserved untouched.

## Implementation Paths
- `src/context/skill_registry.rs` (scan paths, traversal guard, disabled filter)
- `src/api/skills.rs:69-71,184` (call sites — change only if root resolution must move)

## Sub-features
- **Canonical scan path:** `~/.hermes/skills` in defaults.
- **Cycle guard:** visited device+inode set, unit test with A→B→A fixture (<5s).
- **Disabled respect:** parse `skills.disabled`, exclude by name.

## Test References
- `test_registry_scans_hermes_canonical_store`, `test_registry_symlink_cycle_terminates`, `test_registry_honors_disabled_list` (all `tempfile` fixtures, no `$HOME`, no network).

## Known Issues & Notes
- Per-request `reindex()` in `api/skills.rs` is a known perf smell; background refresh is explicitly out of scope (follow-up issue if measured slow).
