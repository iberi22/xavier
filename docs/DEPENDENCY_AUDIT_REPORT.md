# Dependency Security Audit & Vulnerability Report

This report documents the high-severity security vulnerabilities present within Xavier's dependency tree, actions taken to resolve them, and specific blockers/reasons for any remaining dependencies that cannot be immediately upgraded.

---

## Executive Summary

| Dependency | Ecosystem | Advisory/CVE | Severity | Action Taken | Remaining Blocker / Reason |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **jsonwebtoken** | Cargo (Rust) | CVE-2026-25537 (RUSTSEC-2026-XXXX) | **High** | **Upgraded to `10.3` (locked to `10.4.0`)** | None. Crate upgraded successfully and compiles cleanly. |
| **protobuf** | Cargo (Rust) | CVE-2025-53605 (RUSTSEC-2024-0437) | **High** | *Acknowledged & Documented* | Bound by transitive dependencies from the `prometheus` and `opentelemetry-prometheus` crates, which require a major version bump in those parent projects before supporting `protobuf >=3.7.2`. |
| **undici** | npm (JS) | CVE-2026-12151 | **High** | *Acknowledged & Documented* | Brought in transitively by `jsdom` (used by `vitest` in the `panel-ui` frontend). Per task instructions: **"DO NOT touch panel-ui in this PR"**. |
| **opentelemetry_sdk** | Cargo (Rust) | CVE-2026-48504 | **High** | *Acknowledged & Documented* | Transitive dependency of `autometrics v0.3.3` which is pinned to `opentelemetry 0.18.0`. Pushing to `opentelemetry_sdk >=0.32.1` represents a breaking major workspace refactor. |
| **serde_with** | Cargo (Rust) | (Transitive of old `teloxide` features) | **High** | *Acknowledged & Documented* | Pinned transitively to older macro versions via `teloxide v0.12.2`'s feature flags. Upgrading `teloxide` introduces major compiler and architecture churn. |
| **rustls-webpki** | Cargo (Rust) | RUSTSEC-2026-0098, RUSTSEC-2026-0099, RUSTSEC-2026-0104 | **High** | **Upgraded to `0.103.13`** | Updated `rustls-webpki` to its latest secure version (`0.103.13`) under its cargo tracking branch. Older nested `rustls-webpki@0.101.7` is pinned by legacy `reqwest v0.11` via `teloxide`. |

---

## Detailed Vulnerability & Blocker Analysis

### 1. `jsonwebtoken` (RESOLVED)
* **Vulnerability:** Type confusion in claim validation logic (CVE-2026-25537). Malformed standard claims could bypass token expiration or audience checks.
* **Resolution:** Upgraded from `jsonwebtoken = "9.3"` to `jsonwebtoken = "10.3"` (resolving to version `10.4.0` in `Cargo.lock`). The API for Xavier's JWT-validation modules in `src/security/auth.rs` and `src/auth2/jwt.rs` is backward-compatible and builds perfectly.

### 2. `protobuf` (BLOCKED)
* **Vulnerability:** Uncontrolled recursion in `skip_group` decoding causing Stack Overflow Denial of Service (RUSTSEC-2024-0437 / CVE-2025-53605).
* **Vulnerability Path:**
  ```text
  protobuf v2.28.0
  ├── opentelemetry-prometheus v0.11.0
  │   └── autometrics v0.3.3
  │       └── xavier v0.12.0
  └── prometheus v0.13.4
      ├── autometrics v0.3.3
      └── opentelemetry-prometheus v0.11.0
  ```
* **Blocker Reason:** Upgrading `protobuf` to `3.7.2` introduces massive breaking changes to both code generation and runtime APIs. These changes are incompatible with the `prometheus` and `opentelemetry-prometheus` versions consumed by `autometrics v0.3.3`. These parent crates must first receive upstream releases to adopt `protobuf v3`.

### 3. `undici` (BLOCKED)
* **Vulnerability:** Denial of Service (CVE-2026-12151) via unbounded fragment queues.
* **Vulnerability Path:**
  ```text
  undici@7.27.2
  └── jsdom@29.1.1
      └── vitest@4.1.8
          └── panel-ui/package.json (xavier-panel-ui)
  ```
* **Blocker Reason:** `undici` is an npm dependency of `jsdom` inside the React web application `panel-ui`. As explicitly outlined in the PR directive: **"DO NOT touch panel-ui in this PR"**. Thus, any JS/npm adjustments are deferred.

### 4. `opentelemetry_sdk` (BLOCKED)
* **Vulnerability:** Unbounded memory allocation in W3C Baggage propagation (CVE-2026-48504).
* **Vulnerability Path:**
  ```text
  opentelemetry_sdk v0.18.0
  └── autometrics v0.3.3
      └── xavier v0.12.0
  ```
* **Blocker Reason:** `opentelemetry_sdk` is brought in by `autometrics v0.3.3`. Pinned versions of autometrics' exporter endpoints enforce compatibility with the legacy `opentelemetry 0.18.0` SDK models. Upgrading `opentelemetry` to `>=0.32` requires completely re-architecting metrics gathering and rewriting observability hooks across the repository.

### 5. `serde_with` (BLOCKED)
* **Vulnerability:** High severity security vulnerabilities in early `serde_with` macros packages.
* **Vulnerability Path:**
  ```text
  serde_with_macros v1.5.2
  ├── teloxide v0.12.2
  │   └── xavier v0.12.0
  └── teloxide-core v0.9.1
      └── teloxide v0.12.2
  ```
* **Blocker Reason:** Pinned directly by `teloxide v0.12.2`. Upgrading `teloxide` introduces major API changes that break Xavier's Telegram bot wrapper. The modern `serde_with` (v3.21.0) is already used elsewhere in the workspace (e.g. by `tauri-utils`), so the legacy `1.5.2` macro crate remains strictly isolated under the Telegram module.

### 6. `rustls-webpki` (PARTIALLY RESOLVED)
* **Vulnerability:** Wildcard verification constraints bypasses and reachable CRL panic (RUSTSEC-2026-0098, RUSTSEC-2026-0099, RUSTSEC-2026-0104).
* **Resolution:** Upgraded `rustls-webpki` to `0.103.13` within the modern rustls trunk.
* **Remaining Nesting:** An older `rustls-webpki v0.101.7` remains present because it is a downstream requirement of `rustls v0.21.12`, which is pulled in by `reqwest v0.11.27` via `teloxide-core v0.9.1` (used only when the `telegram` feature is enabled). Since `teloxide` is pinned to these older dependencies, compiling the Telegram bot forces the coexistence of both webpki trunks.

---

*Document compiled on: June 2026*
