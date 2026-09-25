# FEATURE: SkillLoader Frontmatter Validation or Removal

**Status:** `implemented` (Implemented 2026-09-21 (kept: executor depends on Skill; format test)) | **Score:** context::skills 3 pass | **Last Tested:** 2026-09-21

## Overview
`SkillLoader` (`src/context/skills.rs`, 100 lines) gates loading on `"# Purpose"` / `"## Purpose"` markers (`validate_skill`, lines 60-64). Real `SKILL.md` files use YAML frontmatter (`name:`/`description:`); a 2026-09-20 probe over the 349-skill store found zero markers — effective load count 0 — and no production callers exist. This feature forces the decision: fix the validator to the real format or delete the module and migrate coverage to the registry.

## Architecture & Design
- **Option A:** mirror `skill_registry::parse_frontmatter` (dir-name fallback), probe test over realistic fixtures returns >0, garbage still rejected.
- **Option B:** remove `skills.rs` + `pub mod` in `src/context/mod.rs`; unique coverage moves to registry tests.
- Implementer probes first (`rg SkillLoader`, 5 frontmatter samples) and documents A-vs-B with evidence. No half-fix.

## Implementation Paths
- `src/context/skills.rs` (fix or delete)
- `src/context/mod.rs` (module registration under Option B)

## Sub-features
- **Probe:** caller list + frontmatter sample, quoted in PR.
- **Resolve:** exactly one of fix/remove; workspace tests green either way.

## Test References
- Option A: `load_all` fixture probe (>0) + garbage rejection. Option B: zero `SkillLoader` references + registry tests green.

## Known Issues & Notes
- Touches nothing outside `src/context/`. Smallest issue in the wave; safe parallel work with #301/#302.
