---
title: "CHORE: Wire or Clean up Enterprise HTTP Routing dead code"
labels:
  - refactor
  - cleanup
assignees: ["jules"]
protocol_version: 1.3.0
---

## Descripción

The compiler outputs several `#[warn(dead_code)]` warnings for unused enterprise HTTP endpoint functions in the `src/enterprise/http.rs` module.

Specifically, the following handlers are fully implemented but never registered in the active routing table or wired into production server routing:
- `create_tenant`
- `list_tenants`
- `get_tenant`
- `create_key`
- `list_keys`
- `revoke_key`
- `query_audit`
- `get_rate_limits`
- `update_rate_limits`

### Goal
We want to either:
1. Wire these handlers into the enterprise feature routes in the server routing module (`src/adapters/inbound/http/routes.rs` or `src/server/http.rs`), or
2. Scope them behind proper feature gates (`#[cfg(feature = "enterprise")]`), or
3. Put `#[allow(dead_code)]` with a descriptive note if they are reserved for future API expansion.

---

## Criterios de Aceptación

- [ ] Clean up or wire the nine unused enterprise handler functions in `src/enterprise/http.rs`.
- [ ] Ensure that running `CARGO_TARGET_DIR=target_local cargo check` or `cargo build` does not emit any dead-code compiler warnings for these functions.
- [ ] No regressions in core HTTP API routing.

---

## Contexto Adicional

- File locations:
  - `src/enterprise/http.rs`
  - `src/adapters/inbound/http/routes.rs`
  - `src/server/http.rs`
- You can run the compiler check locally with:
  ```bash
  CARGO_TARGET_DIR=target_local cargo check
  ```
- Target assignee: `@jules`
