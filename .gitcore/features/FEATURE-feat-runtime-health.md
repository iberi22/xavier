# FEATURE: Runtime Health & Self-Monitoring

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
A continuous self-monitoring framework that runs on a native background loop. It monitors system metrics, SQLite database fragmentation, active embedding providers, local model reachability, and network peer connections, exposing a comprehensive `/health` payload.

## Architecture & Design
The `HealthRegistry` aggregates dynamic diagnostic results from multiple components. It performs disk checks, monitors database size, triggers automatic `VACUUM` queries if page fragmentation exceeds 30%, and validates offline models via lightweight TCP handshakes on Ollama ports with a 500ms timeout. Real-time usage metrics are compiled by atomic counters and displayed on the UI.

## Implementation Paths
- `src/health/` (health registry, system metric collectors, and database checkers)
- `src/cli/handlers/offline_models.rs` (local LLM/embedding reachability and status)
- `src/observability/usage_counters.rs` (real-time request, token, and cost trackers)

## Sub-features
- **System Metrics Monitoring:** Captures host CPU, RAM, and disk utilization.
- **Database Self-Healing:** Measures SQLite bloat and triggers auto-VACUUM dynamically.
- **Embedding & LLM Connectivity:** Active checks for third-party endpoints and local Ollama nodes.
- **Mesh Connection Auditing:** Validates connected peer health, SWAL token states, and sync lags.
- **HTTP Health Endpoints:** Exposes `/health` and `/v1/system/health` to feed the client-side UI.

## Test References
- `test_health_registry_init` and `test_health_registry_singleton` verifying registry setup.
- `test_collect_health_returns_valid_structure` asserting valid JSON outputs.
- `test_health_check_disk_pass` verifying storage bounds.

## Known Issues & Notes
- Integrated cleanly with the `UsageCounters` module to report live costs, error rates, and fallback metrics.
