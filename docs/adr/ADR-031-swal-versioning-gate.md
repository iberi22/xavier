# ADR-031: SWAL Versioning Gate and Preflight Release Verification

*Status: ACCEPTED | Date: 2026-09-01 | Deciders: SWAL Ecosystem Architecture Guild*

---

## Context

During initial ecosystem development, various components in the Xavier codebase and SWAL ecosystem experienced version drift. Manifest files across Rust crates (`Cargo.toml`), frontend packages (`package.json`), and desktop shell configs (`tauri.conf.json`) contained mismatched version strings (e.g., `0.0.1`, `0.6.1-beta`, and premature `1.0.0` releases).

Prematurely bumping major versions to `1.0.0` before complete verification of all wave requirements created confusion regarding contract stability and public API guarantees. Subsequently, premature `1.0.0` releases were reverted back to pre-1.0 release alignment (`0.0.1` / `0.y.z`).

To prevent version desynchronization, accidental breaking changes, and unverified major releases, a formal versioning protocol and automated preflight gate mechanism are required across all repositories in the SWAL ecosystem.

---

## Decision

We establish `docs/SWAL/VERSIONING.md` (and its local repository mirror `docs/SWAL_VERSIONING.md`) as the single canonical source of truth for version management and release gates.

### 1. Semantic Versioning & Pre-1.0 Strategy
- All pre-production software operates strictly under Semantic Versioning 2.0.0 (`0.y.z`).
- Breaking changes in pre-1.0 development bump the MINOR version (`0.y.0`).
- Patch releases, bug fixes, and non-breaking features bump the PATCH version (`0.y.z`).
- Bumping to major version `1.0.0` is prohibited until all ecosystem acceptance criteria, wave features, and public release gates are 100% verified.

### 2. Automated Preflight Gate via `swal-preflight`
We integrate `swal-preflight check` as a mandatory validation gate in local development scripts and CI pipelines:
- **Manifest Synchronization**: Automatically checks and enforces version parity across `Cargo.toml`, `package.json`, and `tauri.conf.json`.
- **Changelog Integrity**: Enforces Keep a Changelog formatting and verifies the existence of a non-empty `[Unreleased]` section before allowing wave completion or release tag creation.
- **Git Safety Check**: Rejects version bump operations if uncommitted changes or dirty workspace states are detected.

### 3. Conventional Commit-Driven Bumping
Version increments are driven by Conventional Commits (`feat:`, `fix:`, `refactor:`, `BREAKING CHANGE:`) using `swal-preflight bump --to <version>`.

---

## Consequences

### Positive
- **Single Source of Truth**: `VERSIONING.md` provides unambiguous rules for versioning across Rust, Node.js, WASM, and Tauri environments.
- **Automated Drift Prevention**: `swal-preflight check` catches version mismatches before commits or pull requests are merged.
- **Traceable Release Readiness**: Ensures `CHANGELOG.md` accurately reflects every wave deliverable under `[Unreleased]` prior to version publication.
- **Accidental Release Prevention**: Guards against premature `1.0.0` bumps until all wave stability criteria pass verification.

### Negative / Trade-offs
- **CI Gate Strictness**: Commits with desynchronized manifests or missing changelog entries will fail CI preflight checks.
- **Tooling Dependency**: Requires `swal-preflight` CLI utility or skill script to be available in build environments.

---

## Alternatives

1. **Manual Version Edits**:
   - *Rejected*: Editing manifests by hand in multiple files is error-prone and caused the original version drift.

2. **Git Tag-Only Versioning**:
   - *Rejected*: Git tags do not automatically propagate into embedded application binaries or npm packages without build-time codegen.

3. **Immediate Unconditional 1.0.0 Bump**:
   - *Rejected*: Declaring `1.0.0` prematurely violates SemVer principles regarding public API stability guarantees during active wave development.
