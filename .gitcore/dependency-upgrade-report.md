# Dependency Upgrade Report

**Date:** 2026-06-10  
**Scope:** Xavier project (Rust backend + frontend/docs)  
**Method:** crates.io API + npm registry + GitHub releases

---

## Rust Crates

| # | Package | Current (Cargo.toml) | Latest Stable | Breaking? | Recommendation |
|---|---------|---------------------|---------------|-----------|----------------|
| 1 | **axum** | `0.8` | `0.8.9` | ⚠️ Minor | **Upgrade** — 0.8.x patch releases. No breaking changes within 0.8. Add WebSocket/HTTP2 features as needed. |
| 2 | **rusqlite** | `0.32.0` | `0.40.1` | ⚠️ Minor | **Upgrade encouraged** — 0.32→0.40 is within 0.x semver. Check CHANGELOG for feature flag changes (default features changed to `cache,ffi-sqlite-wasm-rs`). Newer bundled SQLite. |
| 3 | **sqlite-vec** | *(not in Cargo.toml)* | `0.1.3` (crates.io) | — | **Add dependency** if vector search is needed. Active crate with SQLite vector extension. |
| 4 | **gllm** | `0.10.6` | `0.10.6` | ✅ Same | **No change needed** — latest version matches current. |
| 5 | **tokio** | `1.52.2` | `1.52.3` | ✅ Patch | **Upgrade safe** — minor patch bump. No breaking changes. |
| 6 | **clap** | `4` (caret) | `4.6.1` | ⚠️ Minor | **Upgrade** — 4.x is fully compatible. Resolves to latest 4.6.x with existing spec. |
| 7 | **serde** | `1.0.228` | `1.0.228` | ✅ Same | **No change needed** — latest version matches current. |
| 8 | **ratatui** | `0.29` (optional) | `0.30.1` | ⚠️ Minor | **Upgrade** — 0.30.x introduces new widgets (Chart, LineGauge) but keeps 0.x semver. Check for API changes in event handling. |
| 9 | **crossterm** | `0.28` (optional) | `0.29.0` | ⚠️ Minor | **Upgrade** — 0.29 brings improvements to terminal resize events. 0.x semver compatible. |
| 10 | **chrono** | `0.4` (serde) | `0.4.45` | ✅ Patch | **No change needed** — caret spec already resolves to latest 0.4.x. |
| 11 | **uuid** | `1.8` | `1.23.3` | ✅ Patch | **No change needed** — caret spec resolves to latest 1.x. No breaking changes. |
| 12 | **moka** | `0.12` | `0.12.15` | ✅ Patch | **No change needed** — caret spec resolves to latest. |
| 13 | **governor** | `0.10` | `0.10.4` | ✅ Patch | **No change needed** — caret spec resolves to latest 0.10.x. Last updated Dec 2025; consider if needs are met. |
| 14 | **regex** | `1.10` | `1.11.1` | ✅ Patch | **Upgrade safe** — caret spec should already pull latest. If pinned to 1.10, relax to `1`. |
| 15 | **aes-gcm** | `0.10` | `0.10.3` | ✅ Patch | **No change needed** — caret spec resolves to latest. |
| 16 | **argon2** | `0.5` | `0.5.3` | ✅ Patch | **No change needed** — caret spec resolves to latest. |

---

## Frontend / Node.js Dependencies

| # | Package | Current (installed) | Latest Available | Breaking? | Recommendation |
|---|---------|-------------------|------------------|-----------|----------------|
| 1 | **React** | `19.0.0`-ish (node_modules: `19.3.2`) | `19.2.7` | ✅ Patch | **Upgrade** — minor bump within 19.x. No breaking changes. |
| 2 | **Vite** | `6.2.2` (node_modules) | `8.0.16` | ⚠️ Major | **Evaluate carefully** — v8 is a major jump from v6. Vite 6→7→8 has breaking changes (Rolldown-based, config changes). Recommend upgrading stepwise (6→7 first). |
| 3 | **Astro** | `6.2.2` (node_modules) | `6.4.6` | ✅ Patch | **Upgrade safe** — minor bump within 6.x. Docs site is pinned to `^6.1.6`. |
| 4 | **Starlight** | `0.38.0` (node_modules, pinned `^0.38.0`) | `0.40.0` | ⚠️ Minor | **Upgrade** — 0.39+ adds new UI components (search, sidebar). Check migration notes for any breaking changes to custom layouts. |
| 5 | **ESLint** | `10.1.0` (node_modules, pinned `^10.1.0`) | `10.4.1` | ✅ Patch | **Upgrade safe** — minor bump within 10.x. Flat config, no breaking changes. |

---

## GitCore Protocol

| Component | Current | Latest | Status |
|-----------|---------|--------|--------|
| GitCore Protocol | `v3.6.1` | — | 🔍 **Not found at `opentitles/gitcore-protocol`** (404). The protocol appears to be internal/private to the Xavier project. No public releases detected. |

The `gitcore` term in Xavier refers to an **internal architecture convention** (see `.gitcore/` directory), not a public protocol. The working assumption is Xavier's own GitCore protocol == the project's `.gitcore/` conventions. No external version tracking needed.

---

## Key Upgrade Actions (Priority Order)

### 🔴 High Priority (known breaking or major changes)
1. **Vite** `6.x → 8.x` — Requires migration. Vite 8 uses Rolldown internally. Significant config and plugin changes.
2. **Starlight** `0.38 → 0.40` — Review changelog for layout/component changes before upgrading.
3. **ratatui** `0.29 → 0.30` — Review new widget API changes.

### 🟡 Medium Priority (safe upgrades, features)
4. **rusqlite** `0.32.0 → 0.40.1` — Newer bundled SQLite version. Check feature flags.
5. **crossterm** `0.28 → 0.29` — Improved event handling.
6. **axum** no change needed (already at latest 0.8.x).

### 🟢 Low Priority (patch bumps, no semver changes)
7. **tokio**, **clap**, **chrono**, **uuid**, **moka**, **governor**, **regex**, **aes-gcm**, **argon2** — All covered by caret spec, no action required.
8. **React**, **Astro**, **ESLint** — Covered by caret spec, no manual action required.

---

## Notes

- **sqlite-vec** (`0.1.3`) is not currently in `Cargo.toml` but exists on crates.io if vector search is needed.
- **gllm** `0.10.6` and **serde** `1.0.228` are already at their latest versions.
- All version data sourced 2026-06-10 20:16–20:26. Verify against actual builds before merging upgrades.
