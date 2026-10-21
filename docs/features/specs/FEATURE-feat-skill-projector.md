# FEATURE: Declarative Skill Manifest and Projector

**Status:** `planned`
**Ledger feature ID:** `feat-skill-projector`

## Overview
Current skill discovery relies on fixed directory structures without selective publication across diverse agent CLIs (Codex, Claude Code, OpenCode, Gemini, agy, Hermes, OpenClaw). This feature introduces a declarative manifest format (TOML v1) and an idempotent, atomic symlink projection engine under `src/context/skill_controller/` with CLI mutation commands (`xavier skills plan|apply|rollback`), ensuring that agent harnesses receive only authorized, deliberate skill views without exposing the entire canonical catalog.

## Architecture & Design
- **Canonical Store Outside Discovery Roots (D1):** Authoritative skills reside in a dedicated Git source outside CLI discovery paths, configured via `XAVIER_SKILL_STORE`.
- **Per-Skill Directory Symlinks (D2):** Publication creates one symlink per selected skill directory, preserving inner references and scripts; whole-directory linking is prohibited. File copying is restricted to measured per-adapter fallbacks.
- **TOML v1 Manifest (D3):** Parsed with the existing `toml` crate (`Cargo.toml:262`). Version 1 requires explicit package-to-target placements; reject globs, implicit install-all patterns, and any placement that links an entire source or discovery directory instead of one selected skill package.
- **CLI Mutation Boundary (D4):** Mutating operations are CLI-only (`xavier skills apply|rollback`; later `compose|release` also remain CLI-only). `xavier skills plan` is the default, read-only operation and writes nothing. REST/MCP may expose read-only drift and usage summaries and retain existing dispatch; reject any mutating REST route or MCP tool, including project, apply, rollback, compose, and release. `apply --plan <file>` revalidates preconditions and package hashes.
- **Atomic Symlink Swap (D9):** Atomic replacement creates `symlink(src, dir/.tmp-<rand>)`, followed by `rename` over destination and an `fsync` on the parent directory.
- **Ownership Journal and Safety:** Reuses the cycle-safe `collect_skill_files` traversal (`src/context/skill_registry.rs:185-241`) with canonical-path containment checks and bounded input. Writes an ahead-of-time journal and backups before mutation. Unlinks only unchanged, controller-owned symlinks; never deletes or overwrites unmanaged regular files or follows links outside owner-supplied allowlists.

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
- `test_accepts_minimal_valid_manifest`, `test_accepts_full_valid_manifest`, `test_manifest_rejections` (`manifest.rs`; TOML v1, rejects unknown fields, globs and whole-directory links).
- `test_projector_mutation_is_cli_only` (`src/api/skills.rs`; no mutating REST route or MCP tool is registered).
- `test_apply_plan_publishes_symlinks_and_second_apply_is_idempotent` (`projector.rs`).
- `test_dry_run_apply_reports_without_touching_disk`, `test_dry_run_is_default_and_reports_without_touching_disk` (`projector.rs`, `policy.rs`).
- `test_symlink_backup_preserves_readlink_without_dereferencing`, `test_atomic_write_creates_and_overwrites` (`backup.rs`).
- `test_apply_rejects_unmanaged_regular_file_collision_without_touching_it` (`projector.rs`).
- `test_rejects_root_escape_traversal_and_package_member_escape`, `test_rejects_empty_allowlist_and_unauthorized_target` (`policy.rs`).

## Known Issues & Notes
- Depends on `feat-skill-store-config` and `feat-skill-registry-unify`. Requires acceptance of ADR-034.
- D14: The implementation paths above must be split into issues that each change only listed files, at most 4 files and 400 changed lines.
