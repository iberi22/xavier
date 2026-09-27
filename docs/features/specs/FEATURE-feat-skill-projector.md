# FEATURE: Declarative Skill Manifest and Projector

**Status:** `planned`

## Overview
Current skill discovery relies on fixed directory structures without selective publication across diverse agent CLIs (Codex, Claude Code, OpenCode, Gemini, agy, Hermes, OpenClaw). This feature introduces a declarative manifest format (TOML v1) and an idempotent, atomic symlink projection engine under `src/context/skill_controller/` with CLI mutation commands (`xavier skills plan|apply|rollback`), ensuring that agent harnesses receive only authorized, deliberate skill views without exposing the entire canonical catalog.

## Architecture & Design
- **Canonical Store Outside Discovery Roots (D1):** Authoritative skills reside in a dedicated Git source outside CLI discovery paths, configured via `XAVIER_SKILL_STORE`.
- **Per-Skill Directory Symlinks (D2):** Publication creates one symlink per selected skill directory, preserving inner references and scripts; whole-directory linking is prohibited. File copying is restricted to measured per-adapter fallbacks.
- **TOML v1 Manifest (D3):** Parsed with the existing `toml` crate (`Cargo.toml:262`). Schema requires explicit placements; globs and implicit install-all patterns are rejected.
- **CLI Mutation Boundary (D4):** Mutation is restricted to the CLI (`xavier skills plan|apply|rollback`). REST and MCP interfaces remain read-only. `plan` is the default and writes nothing; `apply --plan <file>` revalidates preconditions and package hashes.
- **Atomic Symlink Swap (D9):** Atomic replacement creates `symlink(src, dir/.tmp-<rand>)`, followed by `rename` over destination and an `fsync` on the parent directory.
- **Ownership Journal and Safety:** Reuses `collect_skill_files` traversal (`src/context/skill_registry.rs:185-241`). Writes an ahead-of-time journal and backups before mutation. Never deletes or overwrites unmanaged regular files; rejects writes outside target allowlists.

## Implementation Paths
- `src/context/skill_controller/manifest.rs`
- `src/context/skill_controller/projector.rs`
- `src/context/skill_controller/policy.rs`
- `src/context/skill_controller/mod.rs`
- `src/context/mod.rs` (`src/context/mod.rs:18-20,43-47`)
- `src/cli/commands/enums.rs` (`src/cli/commands/enums.rs:33-480`)
- `src/cli/handlers/skills.rs`

## Sub-features
- **`manifest_parser`:** Versioned TOML v1 parser enforcing typed sources, targets, and non-glob placements.
- **`dry_run_planner`:** Default `plan` command emitting immutable JSON plans with package hashes and destination preconditions.
- **`atomic_projector`:** Idempotent `apply` command executing atomic symlink swaps with write-ahead journals.
- **`rollback_engine`:** Safe restoration of backed-up links or removal of orphaned controller-owned links.

## Test References
- `test_manifest_parses_toml_v1_and_rejects_unknown_fields`.
- `test_projector_cycle_fixture_terminates` (reusing `src/context/skill_registry.rs:185-241`).
- `test_projector_idempotence_second_run_zero_changes`.
- `test_projector_dry_run_writes_nothing`.
- `test_projector_backup_and_atomic_swap`.
- `test_projector_unmanaged_regular_file_never_deleted`.
- `test_projector_write_outside_allowlist_rejected`.

## Known Issues & Notes
- Depends on `feat-skill-store-config` and `feat-skill-registry-unify`. Requires acceptance of ADR-034.
