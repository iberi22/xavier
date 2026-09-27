# FEATURE: Skill Drift Audit and Secret Scanner

**Status:** `planned`
**Ledger feature ID:** `feat-skill-drift-audit`

## Overview
Over time, skill definitions drift through broken local file references, invalid frontmatter metadata, version contradictions, and unreachable endpoints. Additionally, skill packages risk leaking credentials into agent contexts. This feature adds read-only package auditing and native secret scanning under `src/context/skill_controller/audit.rs`, emitting redacted audit reports and reviewable diffs without automated source mutation.

## Architecture & Design
- **Native Secret Scanning (D10):** Scans the entirety of every candidate package (`SKILL.md`, `references/`, `scripts/`, `assets/`) and composed output using an in-process `RegexSet` (`Cargo.toml:338`) mirroring patterns from `scripts/check-secrets.sh`. Publication is blocked on any finding. Findings output file location and pattern classification only; matched credential bytes are never printed, logged, or included in reports. Existing canonical secrets are reported for human remediation; they are never automatically edited.
- **Report-Only Drift Auditing (D13):** Audit execution is offline by default and strictly read-only. Generates proposed unified diffs using the `similar` crate for human review.
- **Prohibition of Automatic Canonical Rewrites (D13):** Autonomous and automated source rewrites are strictly prohibited. The auditor never modifies, rewrites, or commits changes to canonical skill sources, secrets, safety policies, or approval configurations. All remediation requires explicit human operator approval.
- **Bounded Offline and Online Checks:** Offline checks run by default. Optional probes require an explicit endpoint allowlist, block external redirects outside the allowlist, and never attach credentials. Unreachable endpoints are reported as `unverified` or `unreachable`, never automatically pruned or deleted.
- **Structural Integrity Validation:** Validates frontmatter schemas and slugs using existing validators (`src/context/skill_registry.rs:244-255`, `src/context/skills.rs:64-90`), confirms local file references exist, and identifies conflicting rule declarations across skills.
- **Read-Only Interfaces (D4):** Accessible via CLI `xavier skills audit`, REST `GET /api/skill/drift`, and MCP `xavier_skill_drift_report`. All interfaces remain strictly read-only; no mutating audit endpoint exists.

## Implementation Paths
- `src/context/skill_controller/audit.rs`
- `src/context/skill_controller/mod.rs`
- `src/cli/handlers/skills.rs` (`xavier skills audit` read-only)
- `src/api/skills.rs` (`GET /api/skill/drift` read-only)
- `src/server/mcp/tools_context.rs` (`xavier_skill_drift_report` read-only)

## Sub-features
- **`secret_scanner`:** In-process `RegexSet` scanner evaluating entire packages and blocking publication on exposed secrets without leaking credential values.
- **`drift_detector`:** Integrity and drift checks across manifest, canonical packages, and active projections.
- **`schema_validator`:** Frontmatter schema, description quality, and local file reference checks.
- **`diff_proposer`:** Generates unified diff proposals using `similar` for human review without modifying sources.

## Test References
- `test_audit_fixture_detects_dead_path_missing_field_and_secret`.
- `test_audit_secret_values_never_printed_or_logged`.
- `test_audit_proposes_diff_without_modifying_source`.
- `test_audit_prohibits_automatic_canonical_rewrites`.
- `test_audit_bounded_probe_blocks_redirects_and_omits_credentials`.

## Known Issues & Notes
- Depends on `feat-skill-store-config` and `feat-skill-projector`.
