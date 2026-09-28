# FEATURE: Configurable Skill Store and Scan Paths

**Status:** `planned`
**Ledger feature ID:** `feat-skill-store-config`

## Overview
Currently, `hermes_store_path` (`src/context/skill_registry.rs:96-98`), `hermes_config_path` (`src/context/skill_registry.rs:101-103`), and `default_scan_paths` (`src/context/skill_registry.rs:106-115`) hardcode `$HOME/.hermes/skills` and `$HOME/.hermes/config.yaml`, with only `HOME` resolved from the environment (`src/context/skill_registry.rs:89-93`). This feature introduces the D5 environment contract for the future canonical store, manifest, and host-owned source and target roots. Unset variables preserve today's scan set until the approved migration.

## Architecture & Design
- **12-Factor Configuration (D5):** The required new environment variables are exactly `XAVIER_SKILL_STORE`, `XAVIER_SKILL_MANIFEST`, `XAVIER_SKILL_SOURCE_ROOTS`, and `XAVIER_SKILL_TARGET_ROOTS`; document all four in `.env.example` when implemented. Source and target roots are owner-supplied allowlists that repository content cannot widen. Add no other skill environment keys.
- **Refactoring Precondition (D7a):** Make the store path configurable through `XAVIER_SKILL_STORE` and the legacy disabled-list configuration path an internal, testable parameter; retain `$HOME/.hermes/config.yaml` as its default until migration. With the variables unset, scan workspace `skills/`, workspace `.agents/skills`, and `$HOME/.hermes/skills` exactly as today; the future dedicated Git store is outside every CLI discovery root.
- **Fail-Open Behavior:** If `HOME` is unset, fail open by scanning workspace-relative paths only (`skills/` and `.agents/skills`).
- **Test Seam Preservation:** Preserve the `with_home` and `with_defaults` constructors (`src/context/skill_registry.rs:275-290`) for deterministic, isolated unit testing without host filesystem dependencies.

## Implementation Paths
- `src/context/skill_registry.rs` (`src/context/skill_registry.rs:89-115`, `src/context/skill_registry.rs:275-290`)
- `.env.example`

## Sub-features
- **`store_override`:** Override canonical skill store location via `XAVIER_SKILL_STORE`.
- **`manifest_selection`:** Select the TOML v1 manifest via `XAVIER_SKILL_MANIFEST`; retain the existing Hermes disabled-list behavior until migration.
- **`root_allowlists`:** Host-supplied source and target allowlists (`XAVIER_SKILL_SOURCE_ROOTS`, `XAVIER_SKILL_TARGET_ROOTS`).
- **`backward_compatibility`:** Defaults replicate current Hermes and both workspace discovery paths when environment variables are unset; no target write root is inferred from repository content.

## Test References
- `test_skill_store_env_override_respected` (verifies `XAVIER_SKILL_STORE` overrides canonical directory).
- `test_skill_roots_host_allowlist_cannot_be_widened_by_manifest` (verifies owner policy bounds source and target roots).
- `test_unset_skill_env_preserves_all_three_scan_roots` (verifies workspace `skills/`, workspace `.agents/skills`, and Hermes discovery).
- `test_unset_home_fails_open_workspace_only` (verifies fallback behavior without `HOME`).
- `test_registry_scans_hermes_canonical_store` (regression verification, `src/context/skill_registry.rs:776-1153`).

## Known Issues & Notes
- Foundational refactor (D7a). Prerequisite for `feat-skill-projector` and migration from the Hermes store.
- D14: Each implementation issue may change only its listed files, at most 4 files and 400 changed lines; split work that exceeds those limits.
