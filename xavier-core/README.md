# xavier-core (Experimental — WIP)

> ⚠️ **STATUS: EXPERIMENTAL — DOES NOT COMPILE. NOT PART OF THE BUILD.**

This crate is an **incomplete refactor in progress**. It duplicates
`src/memory/sqlite_vec_store/` and is **explicitly excluded** from the workspace
(see the note in the root `Cargo.toml`, issue #542).

- It is **NOT consumed** by any crate (`xavier`, `code-graph`, `panel-ui`).
- It is **NOT required** to build or run Xavier.
- `cargo build` / `cargo test` at the workspace root will **not** touch it.

## Why is it here?

It is kept in the repo as a non-destructive work-in-progress workspace so the
refactor work is not lost. Do not rely on it, do not depend on it.

## Roadmap

The plan is to replace this duplicate with a **clean `xavier-wasm` crate**
(portable retrieval/scoring logic without native `rusqlite`), following the
pattern already used in `swal-agent-runner/src/wasm/gestalt-wasm` — see the
design docs under `docs/research/` for the WASM/PWA evaluation.

---

**Do not open issues about this crate failing to compile — it is known and intended.**
