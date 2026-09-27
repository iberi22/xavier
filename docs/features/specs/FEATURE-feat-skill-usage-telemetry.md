# FEATURE: Local Skill Usage Telemetry

**Status:** `planned`

## Overview
Skill maintenance and optimization require empirical usage data to understand which skills are dispatched, delivered, and actively utilized by agent harnesses. This feature introduces local, privacy-preserving lifecycle telemetry under `src/context/skill_controller/telemetry.rs`. It records lifecycle events (`selected`, `delivered`, `invoked`, `completed`) in local SQLite storage via the existing migration framework, enforcing 30-day retention without collecting prompt content or driving automated skill deletion.

## Architecture & Design
- **Local SQLite Storage (D12):** Telemetry records are stored in the local workspace SQLite database via the existing migration system (`src/storage/mod.rs:26`), completely separated from the maintainer-wallet schema (`src/data_commons/telemetry_db.rs:9`).
- **Privacy and Opaque IDs (D12):** Event records capture `{event_id, timestamp, workspace_id, task_id, tool, skill_name, package_hash, stage, outcome, latency_ms, token_estimate}`. Never records raw prompt text, memory fragments, absolute filesystem paths, or credentials.
- **Explicit Lifecycle Stages (D12):** Captures distinct stages: `selected`, `delivered`, `invoked`, and `completed`. Confirmed invocation requires explicit harness acknowledgment with idempotent event IDs; delivery is not assumed to be usage.
- **Fail-Open Non-Blocking Boundary:** Telemetry operations must never block or fail skill dispatch. Queue exhaustion or database errors drop telemetry events with an internal counter while dispatch completes normally.
- **Retention and Safety Policies:** Default 30-day retention with automatic event TTL pruning. Usage summaries are report-only; zero observed invocations cannot be used to justify automatic skill deletion.

## Implementation Paths
- `src/context/skill_controller/telemetry.rs`
- `src/context/skill_controller/mod.rs`
- `src/storage/mod.rs` (`src/storage/mod.rs:26`)
- `src/cli/handlers/skills.rs` (`xavier skills usage`)
- `src/api/skills.rs` (`GET /api/skill/usage` read-only)
- `src/server/mcp/tools_context.rs` (`xavier_skill_usage` read-only)

## Sub-features
- **`event_pipeline`:** Records lifecycle events (`selected`, `delivered`, `invoked`, `completed`) with opaque IDs.
- **`fail_open_buffer`:** Bounded ingestion queue ensuring zero impact on dispatch latency or reliability.
- **`usage_reporting`:** Generates local usage and coverage summaries for CLI, REST, and MCP.
- **`ttl_pruning`:** Enforces 30-day event retention without deleting any skill packages.

## Test References
- `test_telemetry_records_stages_with_opaque_ids`.
- `test_telemetry_fail_open_on_db_error`.
- `test_telemetry_concurrent_events_no_corruption`.
- `test_telemetry_ttl_retention_prune`.

## Known Issues & Notes
- Depends on `feat-skill-registry-unify` and `feat-skill-projector`.
