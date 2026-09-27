# FEATURE: Unified Skill Registry Constructor and Thresholds

**Status:** `planned`

## Overview
Registry construction via `SkillRegistry::with_defaults` and `reindex` is currently duplicated across REST (`src/api/skills.rs:69-71,184-185`), MCP (`src/server/mcp/tools_context.rs:125-133`), and HTTP session fusion (`src/server/http/context.rs:196-198`). Additionally, `MIN_DISPATCH_CONFIDENCE = 0.40` (`src/context/skill_dispatcher.rs:79`) and `SKILL_CONFIDENCE_THRESHOLD = 0.5` (`src/server/http/context.rs:15`) are undocumented magic numbers. This feature establishes a single construction entry point and documents both confidence thresholds to eliminate ranking drift between surfaces.

## Architecture & Design
- **Single Construction Site (D7b):** Extract a unified `build_skill_registry(workspace)` constructor in `src/context/skill_registry.rs` invoked by REST, MCP, and HTTP session fusion, ensuring identical ranking, indexing, and candidate sets across all entry points.
- **Named Threshold Constants (D7c):** Explicitly name, document, and export `MIN_DISPATCH_CONFIDENCE = 0.40` (`src/context/skill_dispatcher.rs:79-85`) for dispatch and `SKILL_CONFIDENCE_THRESHOLD = 0.5` (`src/server/http/context.rs:15`) for Maximum-level session fusion.
- **Interface Compatibility:** REST `GET /skills` and `POST /api/skill/dispatch` (`src/api/skills.rs:61,180`) and MCP `xavier_dispatch_skill` and `xavier_skill_list` (`src/server/mcp/tools_context.rs:99-119`) response shapes remain byte-compatible. Minimal/Medium session fusion remains byte-identical (`src/server/http/context.rs:181-245`).

## Implementation Paths
- `src/context/skill_registry.rs` (shared `build_skill_registry` helper)
- `src/context/skill_dispatcher.rs` (`src/context/skill_dispatcher.rs:79-85`)
- `src/server/http/context.rs` (`src/server/http/context.rs:15,196-198`)
- `src/api/skills.rs` (`src/api/skills.rs:69-71,184-185`)
- `src/server/mcp/tools_context.rs` (`src/server/mcp/tools_context.rs:125-133`)

## Sub-features
- **`unified_constructor`:** Shared `build_skill_registry(workspace)` constructor for REST, MCP, and HTTP fusion.
- **`named_thresholds`:** Documented constants for dispatch (`0.40`) and context fusion (`0.50`).
- **`surface_parity`:** Guarantees count and ranking parity across REST and MCP endpoints.

## Test References
- `test_build_skill_registry_shared_parity` (verifies REST `GET /skills` count matches MCP `xavier_skill_list` count on identical fixtures).
- `test_confidence_calibration_range` (regression verification, `src/context/skill_registry.rs:966-1312`).
- `test_fusion_*` (regression verification across 4 fusion tests, `src/server/http/context.rs`).

## Known Issues & Notes
- Precondition for `feat-skill-projector` and `feat-skill-usage-telemetry`. Depends on `feat-skill-store-config`.
