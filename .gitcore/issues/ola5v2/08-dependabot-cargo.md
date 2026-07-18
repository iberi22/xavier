# [Ola 5v2 · 08] Dependabot: remediate Cargo high-severity crates

> Split of #478 (cargo track). Gold-standard Jules issue.

## Web Research Required (Jules must search the web)

1. **jsonwebtoken Rust advisories** — search: `jsonwebtoken rust type confusion CVE advisory 2024 2025`, read RustSec/crates.io fixed versions.
2. **protobuf recursion vulnerability** — search: `protobuf rust uncontrolled recursion vulnerability fixed version 2024`.
3. **opentelemetry_sdk baggage memory** — search: `opentelemetry_sdk unbounded memory W3C Baggage advisory 2024 2025`, check https://rustsec.org/

For each high alert, cite advisory ID + fixed version in PR.

## Exact Technical Context

- Workspace root `Cargo.toml` / `Cargo.lock`
- Gate: `cargo check --workspace` must stay 0 errors
- Prefer targeted `cargo update -p <crate> --precise <ver>` over blind full update
- Known classes from recent API sample: jsonwebtoken, protobuf, opentelemetry_sdk, serde_with

> CRITICAL: DO NOT modify panel-ui. DO NOT touch xavier-core if excluded. NEVER `.patch` files. Empty PR rejected.

## Problem

High-severity Rust advisories remain open on main (~11 high-class Dependabot findings).

## Acceptance Criteria

- [ ] Inventory table: alert → action (bumped / deferred+reason)
- [ ] `cargo check --workspace` 0 errors
- [ ] Prefer minimal version deltas
- [ ] Document remaining highs

## Files to Modify

| File | Change |
|---|---|
| `Cargo.toml` (workspace/members as needed) | bumps |
| `Cargo.lock` | lock |

## Verification

```bash
cargo check --workspace
```

## Dependencies and Merge Order

- **Depends on:** prefer **after 07**
- **Can run in parallel with:** 06, 05 if no lock conflicts
