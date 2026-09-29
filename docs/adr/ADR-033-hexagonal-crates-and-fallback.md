# ADR-033: Hexagonal crates for agent isolation, with explicit fallback chains

*Status: PROPOSED | Date: 2026-09-29 | Extends: ADR-002*

## Context

Xavier is built in waves by many autonomous agents (Jules, opencode, Claude) working in parallel.
Measured on `main` (2026-09-29):

- 280k lines of Rust in `src/`; only **10.9k (3 %)** live in `domain/`, `ports/`, `adapters/`, `app/`.
- **44 pairs of top-level modules import each other** (non-test code). 36 of them hang on a single file on one side.
- Hubs: `memory` (42k lines) is imported by 35 modules; `server` depends on 41.
- Rule violations: `domain/` imports `adapters/`, `security` and `memory`; `ports/` imports `memory`, `agents`, `session`.
- One crate (plus `xavier-core-logic`, `xavier-wasm`): module boundaries are conventions, not enforced.

Observed cost (wave of 2026-09-29): tasks had to touch files outside their scope (J06 → `projector.rs`,
V10 → `fallback_store.rs`), every merge forced rebases elsewhere, and a hidden "catch-all" fallback in
`ConnectionManager` silently opened the wrong database and caused a startup race (#2736).

Fallbacks exist but are ad hoc and inconsistent (secrets, mesh transport, embeddings, retrieval, health);
some are not wired at all (`FallbackSecretStore::chain_from_env` has no production consumer).

## Decision

1. **Crates are the walls.** Xavier becomes a Cargo workspace with compile-time boundaries:
   - `xavier-domain` — pure types and rules, no I/O, no tokio, no sqlite.
   - `xavier-ports` — traits only (inbound and outbound contracts). Depends only on `xavier-domain`.
   - `xavier-adapter-*` — one crate per connector (e.g. `sqlite-vec`, `embed-ollama`, `embed-openai-compat`,
     `llm-openrouter`, `secrets-vault`, `mesh-http`). Depends on `xavier-ports` (+ its own libs), never on another adapter.
   - `xavier-app` — use cases orchestrating ports.
   - `xavier` (server/cli) — the composition root: the only place that wires adapters to ports.
2. **ADR-002 still decides *whether* a port exists**, with one added criterion: *a boundary another agent
   must not cross* (an isolation need) now counts as one of the two required criteria.
3. **Every outbound port gets an explicit fallback policy**, implemented once as a generic
   `FallbackChain<P>` (ordered adapters, health check, circuit breaker) plus a declared degraded mode:
   embeddings down → BM25-only search; LLM down → no HyDE/rewriting; vector store down → FTS; mesh down →
   local-only with queued sync; primary secret backend down → next durable backend (never a volatile one).
4. **Fallbacks must be explicit and observable.** A fallback never silently changes semantics (no guessed
   paths, no "catch-all" resources). Entering a degraded mode is logged once and reported by `/health`
   as `degraded` with the reason. Tests cover each degraded mode.
5. **One agent per crate.** Each crate has its own `AGENTS.md` (its port, invariants, test command).
   Waves assign issues by crate; tasks in disjoint crates run in parallel. Changing a trait in
   `xavier-ports` is a contract change and needs an ADR or owner review.
6. **Incremental (strangler), never big-bang.** Order: break module cycles (Wave 0) → extract
   `xavier-domain` + `xavier-ports` and the embedding/LLM adapters with their fallback chains (Wave 1) →
   secrets, mesh, storage (later waves) → split `memory` last, in slices.

## Consequences

- Positive: agents cannot break each other's areas across crate boundaries; CI can run `cargo test -p <crate>`
  for the touched crate only; fallbacks become uniform, testable and visible.
- Negative: a multi-wave refactor; more `Cargo.toml` files; ports must be designed carefully because they
  become stable contracts.
- Measure of progress: number of module cycles (target 0), share of code outside the root crate, and one
  `FallbackChain` per outbound port with a degraded-mode test.

## Addendum A — crate map and build/test-time plan (measured 2026-09-29)

**Why build time is the bottleneck.** The root crate is one compilation unit of 280k lines: touching a leaf
module nobody imports (`a2a`) costs ~19 s of `cargo check`, and every test run links one huge binary.
Split crates are compiled in parallel, cached independently, and tested with `cargo test -p <crate>`.

**Extractable now (no cycles, 0–1 internal deps outside utils/settings/error/models/time/domain/ports):**
`a2a`, `curation`, `plugins`, `self_manage`, `system`, `utils`, `clavis`, `consistency`, `consolidation`,
`documents`, `gateways`, `maturity`, `polygon_anchor`, `rag`, `tools`, `verification`, `billing`
(~23k lines). These can be extracted in parallel by independent agents.

**Optional product crates (fan-in 0 — no module imports them), moved behind Cargo features so the default
build and the default test run skip them:** `a2a`, `plugins`, `gateways`, `maturity`, `billing`, `auth2`,
`chronicle`, `telegram`, `api`, `ui` (and later `enterprise`, `telecom`, `humanchallenge`). Nothing is
deleted (Anti-Destruction rule): capabilities stay, they just stop being compiled when not requested.

**Heavy dependencies to gate or replace:**
- `sqlx` (Postgres) is always compiled but used by 4 modules (`agents`, `memory`, `nodes`, `workspace`) → a `postgres` feature/adapter crate.
- `image` is always compiled, used only by `documents` → moves with the `documents` crate.
- `oqs` (liboqs: C + CMake + libclang) → replace with the pure-Rust RustCrypto `ml-kem` / `ml-dsa`, as swal-vault did (VAULT-01/04).
- Already optional and kept so: `candle-*`, `gllm`, `alloy`, `tauri`, `teloxide`.

**Core cluster, extracted last (after Wave 0 removes the cycles):** `crypto`, `mesh`, `node_identity`,
`security`, `secrets`, `agents`, `memory` (42k lines, split in slices), `server`, `cli`.

**Wave plan:**
- Wave 30 (Wave 0): break the 44 module cycles — 15 issues, disjoint file islands, ready.
- Wave 31 (Wave 1a): foundation crates (`xavier-utils`, `xavier-error`, `xavier-settings`) + the 17 leaf crates above.
- Wave 32 (Wave 1b): feature-gate the optional product crates; `postgres` feature; `oqs` → RustCrypto.
- Wave 33 (Wave 2): `xavier-domain` + `xavier-ports`; embedding and LLM adapters with `FallbackChain`.
- Wave 34+: secrets, mesh, storage, then `memory` slices; CI switches to "test only affected crates".

**Complexity reduction without losing functionality:** fewer cross-module edges (44 cycles → 0), one
`FallbackChain` replacing ~6 ad hoc fallback implementations, and a smaller default build — with every
existing capability still available behind its crate/feature.
