# Live Server Clearance E2E Testing Notes

This document describes how to execute the end-to-end clearance tests against a live Xavier server binary and explains why the suite is opt-in.

---

## Overview

The `tests/clearance_live_e2e.rs` integration test suite validates security clearance enforcement against a running Xavier HTTP server process (`xavier http <PORT>`).

Unlike unit and mock middleware tests, `clearance_live_e2e` spawns the actual binary built by Cargo (`CARGO_BIN_EXE_xavier`), binds to an ephemeral local port, boots the HTTP router with a temporary workspace, and drives real HTTP requests over TCP using `reqwest`.

---

## Why the Suite is Opt-In

Spawning child processes and booting full database/embedding stores introduces significant overhead:
- Binary compilation requirements
- Port binding and OS process management
- Temporary workspace filesystem I/O

To keep standard `cargo test` runs fast and prevent flaky failures in restricted CI environments, the suite is guarded by the environment variable:

```bash
XAVIER_LIVE_E2E=1
```

Without `XAVIER_LIVE_E2E=1`, the test cases log a skip message and return immediately, resulting in 0 test failures.

---

## How to Run Locally

To run the live server clearance E2E suite locally:

```bash
XAVIER_LIVE_E2E=1 cargo test --package xavier --test clearance_live_e2e --features ci-safe -- --test-threads=1
```

### Running without the environment variable (default):

```bash
cargo test --package xavier --test clearance_live_e2e --features ci-safe -- --test-threads=1
```
*Outputs 0 failed tests (tests skip cleanly).*

---

## Test Coverage Summary

1. **Case A (`test_clearance_live_search_ceiling`)**:
   - Seeds UNCLASSIFIED and SECRET documents via Admin token.
   - Executes `POST /memory/search` using a Readonly JWT token.
   - Verifies the Readonly requester cannot view unredacted SECRET document content and `hidden_by_clearance > 0`.
   - Executes the same query as Admin and confirms full access.

2. **Case B (`test_clearance_live_route_policy_and_audit`)**:
   - Configures route-level policy via `XAVIER_REQUIRED_CLEARANCE_ROUTES={"routes":[{"prefix":"/segments","required":"SECRET"}]}`.
   - Verifies a Readonly requester targeting `/segments/x` receives `403 FORBIDDEN`.
   - Verifies an Admin requester targeting `/segments/x` receives `200` or `404` (never 403).
   - Confirms denial and access events appear in `XAVIER_CLEARANCE_AUDIT_PATH`.

3. **Case C (`test_clearance_live_export_ceiling_and_audit`)**:
   - Seeds TOP_SECRET memory content.
   - Calls `GET /memory/export` with Readonly authority.
   - Confirms exported payload does not exceed requester's clearance ceiling.
   - Confirms audit log file records `"action":"export"`.
