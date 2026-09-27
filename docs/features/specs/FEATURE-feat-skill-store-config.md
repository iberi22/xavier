# FEATURE: Configurable Skill Store and Scan Paths

**Status:** `planned`

## Overview
Currently, `hermes_store_path` (`src/context/skill_registry.rs:96-98`), `hermes_config_path` (`src/context/skill_registry.rs:101-103`), and `default_scan_paths` (`src/context/skill_registry.rs:106-115`) hardcode `$HOME/.hermes/skills` and `$HOME/.hermes/config.yaml`, with only `HOME` resolved from the environment (`src/context/skill_registry.rs:89-93`). This feature introduces 12-factor configuration overrides for the canonical skill store, configuration source, scan paths, and target allowlists, with defaults that reproduce today's scan set exactly.

## Architecture & Design
- **12-Factor Configuration (D5):** Add environment variable overrides: `XAVIER_SKILL_STORE` (default `$HOME/.hermes/skills`), `XAVIER_SKILL_CONFIG` (default `$HOME/.hermes/config.yaml`), `XAVIER_SKILL_MANIFEST`, `XAVIER_SKILL_SOURCE_ROOTS`, and `XAVIER_SKILL_TARGET_ROOTS`. All variables documented in `.env.example`.
- **Refactoring Precondition (D7a):** Make store and config paths configurable prior to projector development, keeping existing behavior as the baseline.
- **Fail-Open Behavior:** If `HOME` is unset, fail open by scanning workspace-relative paths only (`skills/` and `.agents/skills`).
- **Test Seam Preservation:** Preserve the `with_home` and `with_defaults` constructors (`src/context/skill_registry.rs:275-290`) for deterministic, isolated unit testing without host filesystem dependencies.

## Implementation Paths
- `src/context/skill_registry.rs` (`src/context/skill_registry.rs:89-115`, `src/context/skill_registry.rs:275-290`)
- `.env.example`

## Sub-features
- **`store_override`:** Override canonical skill store location via `XAVIER_SKILL_STORE`.
- **`config_override`:** Override disabled skill configuration file via `XAVIER_SKILL_CONFIG`.
- **`root_allowlists`:** Host-supplied source and target allowlists (`XAVIER_SKILL_SOURCE_ROOTS`, `XAVIER_SKILL_TARGET_ROOTS`).
- **`backward_compatibility`:** Defaults replicate current Hermes and workspace discovery paths when environment variables are unset.

## Test References
- `test_skill_store_env_override_respected` (verifies `XAVIER_SKILL_STORE` overrides canonical directory).
- `test_unset_home_fails_open_workspace_only` (verifies fallback behavior without `HOME`).
- `test_registry_scans_hermes_canonical_store` (regression verification, `src/context/skill_registry.rs:776-1153`).

## Known Issues & Notes
- Foundational refactor (D7a). Prerequisite for `feat-skill-projector` and migration from the Hermes store.
