# FEATURE: Dual License (MIT + Mesh License)

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
Xavier operates under a dual-licensing scheme. Standalone and purely local operations are governed under the permissive MIT license. Activating network capabilities, peer-to-peer workspace sharing, the governance DAO, and the Data Commons reward system requires accepting the Mesh License terms.

## Architecture & Design
The system evaluates features against the active `LicenseKind`. If a mesh-level feature (like multi-node shared workspace joins or DAO voting) is requested but the Mesh License has not been accepted, the runtime blocks execution, ensuring strict regulatory and license compliance.

## Implementation Paths
- `src/security/license.rs` (LicenseKind enum, accept logic, and runtime license gates)
- `src/cli/handlers/license.rs` (CLI commands for acceptance and queries)

## Sub-features
- **License State Tracking:** Enumerates standard MIT vs Mesh License kinds.
- **Interactive CLI Accept:** CLI handlers allowing users to view, accept, or query license status.
- **Runtime Gates:** Blocks peer and network functions unless `settings.license.mesh_accepted` is `true`.
- **License Auditing:** Securely writes license choices to localized settings without external network calls.

## Test References
- `test_default_is_mit` asserting standalone MIT is default.
- `test_accept_upgrades_to_mesh` confirming transition states.
- `test_require_fails_without_acceptance` validating feature gating.
- `test_duplicate_accept_returns_false` checking state transitions.

## Known Issues & Notes
- Gating is completely local and does not transmit telemetry data to any central authority, maintaining 100% user privacy.
