# Core refactor plan (2026-10-29)

Status: proposed. Baseline tag: `v0.2.19-stable-pre-refactor` (commit `5df6e264d`, #3584; the build deployed locally as xavier 0.2.19, all main workflows green on that commit).

This plan applies [ADR-019 (plugin-first boundary)](../adr/ADR-019-plugin-first-boundary.md) to the code that exists today. The goal for 1.0 is a core that is 100% functional and verified, with the rest of the surface either optional (cargo feature, off by default) or outside the core crate (sidecar or separate crate). It does not change any ledger status by itself; statuses move only through the verifier and the Ledger Promoter.

## 1. Why

Measured on `main` at `44925eaa9` (2026-10-29):

- Root crate `src/`: about 408k lines of Rust in 75 public modules; `src/lib.rs` carries crate-wide `#![allow(dead_code)]` and `#![allow(unused_imports)]`, so the compiler does not report unused code.
- ADR-019 says code-graph, mesh and panel live outside the core. Today `code_graph` is a direct dependency held in `AppState` (`code_indexer`, `code_query`, `code_db`), mesh is always compiled, and `src/health/mod.rs` uses `crate::mesh::MeshMaturityReport`.
- Mesh, data_commons, maloca, telecom, governance and polygon_anchor are always compiled. `src/cli/server.rs` mentions maloca 36 times; `AppState` holds the telecom registry.
- Some cargo features have no `cfg` use (`dev-mode`) or a single one (`libp2p`).
- Ledger (`.gitcore/features.json`): 117 entries, 70 stable. 2026-10-28 implementation audit: 56% weighted, with mesh at 29% and cloud at 43%.

Splitting effort across non-core areas keeps the core below 100%. Usage and build-time costs of each area were not measured; treat those as hypotheses, not findings.

## 2. Core 1.0 definition

In the core (must be 100% functional, with integration/E2E tests the promoter can count):

| Area | Scope |
|---|---|
| Memory | write, search (FTS + vector), update, delete, integrity, migrations |
| Storage | SQLite/sqlite-vec store, backup and recovery |
| Embeddings / LLM | provider abstraction, local and remote providers |
| MCP | server, tools/list, memory tools, auth |
| CLI | daemon lifecycle, memory commands, config, `--json` output |
| Config / vault | settings, secrets at rest, key policy |
| Node identity / replication records | as ADR-019 keeps them in core |

Out of the core (optional feature, sidecar, separate crate or private repo): mesh (+ iroh/libp2p transports, tokenomics), data_commons, maloca, telecom, governance and polygon_anchor, market/billing/enterprise, training/humanchallenge, code-graph runtime, panel.

## 3. Cut list (ledger view)

These ledger IDs move out of the core 1.0 denominator. They are not deleted; P1.5 gives them a `deferred` status.

- Set A (16): feat-mesh-network, feat-governance-dao, feat-mesh-service-network, feat-mesh-private-wallet, feat-maloca, feat-maloca-roadmap-route, feat-maloca-governance-rules, feat-maloca-governance-wiring, feat-maloca-store-atomic-persist, feat-node-provisioning, feat-ivn-api, feat-ivn-karma, feat-ivn, feat-marketplace-api, feat-data-node-consent, feat-mesh-replay-store. Effect: 65/101 stable (64.4%).
- Set B (A + 11): feat-training-datasets-api, feat-challenge-curation-pipeline, feat-human-introspection-mode, feat-human-curation, feat-mini-experts, feat-wasm, feat-generative-ui, feat-autonomous-delivery, feat-telegram-bot, feat-auto-improvement, feat-improve-bounded. Effect: 57/90 (63.3%).

Set A is the default proposal. Set B needs an owner decision.

## 4. Phases

Rule for every phase: one PR = one change; builds with default features and with each gated feature in CI; existing tests pass; core integration/E2E tests are kept or added and registered in the ledger.

### P0: hygiene (parallel)

- Remove the `gateways` module (`src/gateways/`, no importers outside itself).
- Remove the empty `dev-mode` cargo feature and the `libp2p` feature with its single `cfg` block (Iroh is the transport). `self-manage` stays (part of the `headless` and `ci-safe` bundles).
- Remove the unmounted `server::gpud_sidecar` module and its test (`tests/test_gpud_sidecar.rs`, suite_12 and the e2e-segmented filter). ADR-013 gets a note that the sidecar will come back as a separate binary.
- Replace the crate-wide `allow(dead_code)` / `allow(unused_imports)` with per-module allows, so cleanup can go module by module (sequential: touches `src/lib.rs`).
- Already covered elsewhere: index-self (#3616, merged), maloca models list (in-flight Jules task x1). `a2a` stays: `chronicle::auto_docs` uses it.

### P1: decouple, then feature-gate (`src/cli/server.rs`, `src/lib.rs`/`AppState` and `Cargo.toml` changes are sequential)

1. Health without mesh: `src/health` must not import `crate::mesh`; the mesh maturity report is provided through a trait or an optional field filled by the mesh module.
2. Maloca wiring out of `src/cli/server.rs`: a single `maloca` wiring function or module that `server.rs` calls once.
3. Telecom out of `AppState`: registered via an optional extension instead of a hard field.
4. Feature-gate, one PR each, off by default: `mesh` (reuse the existing `mesh` feature), `data-commons` (reuse the existing feature), `maloca`, `telecom`, `governance` (+ `polygon_anchor`). Each PR adds a CI job building and testing with that feature on.

### P1.5: `deferred` ledger status

Add `deferred` to the ledger status vocabulary (schema, validators, `scripts/ledger-promote.py`, tests). Deferred entries are excluded from the stable-percentage denominator and are never promoted or demoted. Then a separate data-only PR marks set A as `deferred`.

### P2: crate extraction (sequential)

1. `xavier-crypto` / vault
2. `xavier-llm` (providers, embeddings client)
3. `xavier-store` (memory store traits and SQLite backends): after 1 and 2
4. `xavier-mcp`: after store

Each extraction moves code without behaviour changes, keeps a re-export shim in the root crate for one release, and keeps the same tests passing.

### P3: code-graph sidecar-only

`AppState` drops `code_indexer` / `code_query` / `code_db`; the daemon talks to the code-graph binary through the existing versioned JSON-over-stdio/UDS contract (`code-graph/src/plugin/engine.rs`). MCP exposure only where an agent consumes it.

### P4: billing / enterprise / market (plan only)

Move to the private core repo. No work in this repo beyond defining the plugin boundary.

## 5. Plugin model

- In-process optional code: workspace crates + cargo features.
- Optional runtime pieces: sidecars with a versioned contract (JSON over stdio or UDS), MCP only when an agent consumes it.
- No dynamic `.so` plugins.

## 6. Done criteria for the refactor

- `cargo build` / `cargo test` with default features: no mesh, maloca, telecom, data_commons or governance code compiled.
- CI matrix: default, `headless`, and each gated feature.
- Ledger: set A `deferred`; core features stable with integration/E2E evidence on a green main SHA.
- Rollback point: `v0.2.19-stable-pre-refactor`.
