# Xavier Implementation Audit (2026-10-28)

Measured on 2026-10-28 against `main` at `5b690c131` (refreshed on 2026-10-30 against current `main`). This is a dated, hand-made audit snapshot, not a release claim and not a ledger artifact.

The source of truth for feature status remains `.gitcore/features.json`, promoted only by `scripts/verify-pipeline.sh` and the Ledger Promoter (see `AGENTS.md` sections 4 and 5). This audit does not change any ledger status. It records where the code, the ledger and the release snapshots disagree, so those gaps can be closed through the normal verification lanes.

## Summary

- **Overall (weighted, code evidence): 57%**
- Unweighted feature average: 56%
- Features inventoried: 100 (31 implemented, 44 partial, 13 stub, 12 missing)

| Area | Weight | Features | Implemented | Partial | Stub | Missing | Score |
|---|---:|---:|---:|---:|---:|---:|---:|
| Memory core | 15 | 9 | 3 | 4 | 1 | 1 | 58% |
| Code indexing (code-graph) | 10 | 10 | 2 | 8 | 0 | 0 | 60% |
| MCP surface | 10 | 7 | 2 | 3 | 2 | 0 | 56% |
| CLI parity | 10 | 12 | 5 | 5 | 1 | 1 | 64% |
| LLM and provider configuration | 7 | 6 | 2 | 2 | 1 | 1 | 53% |
| Config and runtime operations | 8 | 8 | 4 | 4 | 0 | 0 | 75% |
| Vault, recovery, backup and USB | 8 | 7 | 1 | 4 | 1 | 1 | 46% |
| Multi-tenant and enterprise | 6 | 5 | 1 | 2 | 1 | 1 | 44% |
| Agent onboarding, skills and sessions | 6 | 6 | 3 | 2 | 1 | 0 | 70% |
| Cloud, SaaS and billing | 6 | 7 | 2 | 2 | 0 | 3 | 43% |
| Mesh, P2P, IVN and Maloca | 6 | 12 | 0 | 5 | 5 | 2 | 29% |
| Panel, desktop and packaging | 4 | 5 | 3 | 1 | 0 | 1 | 70% |
| Docs, CI and release | 4 | 6 | 3 | 2 | 0 | 1 | 67% |
| **Overall (weighted)** | 100 | 100 | 31 | 44 | 13 | 12 | **57%** |

### Against the GitCore ledger (`.gitcore/features.json`, 117 entries)

| Measure | Value |
|---|---:|
| Ledger metadata `overall_progress_pct` (last updated 2026-10-02, counts 101 entries) | 90.0% |
| Pipeline `real%` (mean `progress_pct` over all 117 entries) | 76.9% |
| Entries `stable` / `beta` / `in_progress` / `planned` | 74 / 16 / 2 / 25 |
| `progress_pct` share backed by passing integration or E2E tests | ~22% |
| `progress_pct` share backed by passing E2E tests only | ~9% |

The E2E figures map each entry's declared `tests[]` to integration targets under `tests/` and to E2E suites; they are an estimate, not the promoter's own calculation. Most `stable` entries (60 of 74) declare only unit-level tests, and 64 of 74 were last tested before 2026-09-28. Four `stable` entries still carry `open_gaps`. Merged explain features (WAVE-41 to WAVE-43) remain `planned` because the Ledger Promoter has not run on `main` since 2026-10-21.

## Method

1. **Sources.** README, `docs/` (feature specs, SRS, KNOWN_ISSUES, MCP contract, CLI reference), the feature ledger `.gitcore/features.json` (117 entries), the CLI parity audit of 2026-10-27 (CLI commands, MCP tool lists, live-node findings), merged and open pull requests and issues, and a source scan for `501` responses and "not implemented" paths.
2. **Status per feature.**
   - `implemented` = 1.0: merged, reachable from a public surface, and covered by tests or a merged PR.
   - `partial` = 0.5: works, but is beta, in progress, unverified, or missing part of its scope.
   - `stub` = 0.2: the surface exists (command, route, tool) but returns a placeholder, `501`, or a static value.
   - `missing` = 0.0: specified or planned, with no working code on `main`.

   Work that is open in a pull request or an agent session but not merged counts as `partial` if part of the feature already works, and `missing` otherwise.
3. **Area score** = mean of its feature scores.
4. **Overall** = area scores weighted by product importance (weights sum to 100). Memory, code-graph, MCP and CLI carry the most weight because they are the core product surfaces.
5. The inventory was cross-checked against a second, independent read-only code review. Rows were changed only where the code on `main` confirmed the difference.
6. Security hardening items are tracked privately and are not listed or scored here.

To re-measure, update the status of a row and recompute. No build is needed.

## Scope notes

- **Cloud / SaaS.** The hosted cloud and billing back-ends are a separate private service. This document scores only the client side in this repository (plan catalog, `xavier billing`, cloud sync) and whether it is wired to the hosted service.
- **Mesh / IVN / Maloca** carry a lower weight because most of the area is experimental and off by default.

## Inventory by area

### Memory core: 58% (weight 15)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Add / search / recall with hybrid retrieval (HTTP, CLI, MCP) | implemented | `src/server/mcp/tools_memory.rs`, `docs/MCP_CONTRACT.md`; ledger stable | Keep contract tests green |
| PageIndex tree retrieval | implemented | `crates/xavier-pageindex`, README PageIndex section | - |
| Explain (memory / code-graph sources, isolated LLM, render route) | implemented | #3536, #3537, #3545, #3561, #3562 | Ledger still lists these as `planned`; update the ledger |
| Memory write integrity | partial | ledger `feat-memory-write-integrity` = beta | Promote to stable with long-run evidence |
| Encryption at rest | partial | ledger `feat-encryption-at-rest` = beta; README: auxiliary stores still migrating | Finish auxiliary store migration and validate against long-lived data |
| Per-repo memory instance (XAV-REPO) | partial | ledger `feat-repo-memory-instance` = beta | Stabilise config/privacy semantics, add e2e test |
| Memory stats through the port interface | stub | `src/adapters/inbound/http/handlers/memory.rs` returns "not yet implemented in port interface" | Implement stats on the storage port |
| FTS index rebuild | missing | `docs/features/specs` fts-index-rebuild = draft-analysis | Design and implement rebuild command |
| Isolation of test data from live stores | partial | CLI parity audit P2-5 (not started) | Hermetic test fixtures; no test users in live stores |

### Code indexing (code-graph): 60% (weight 10)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Native parsers (Rust, TypeScript, Python, Go, Java, C, C++) | implemented | `code-graph/Cargo.toml` feature `native-parsers`, `code-graph/src/parser/*` | - |
| Live index coverage and freshness | partial | CLI parity audit: live index observed Rust-only and stale | Multi-language reindex, freshness signal in health |
| `xavier code repos` management | partial | CLI parity P1-1, in progress | Land repo list/add/remove with `--json` |
| Graph dump path handling | partial | ledger `feat-codegraph-dump-path` = beta | Promote with tests |
| Code-graph optimisations O1-O7 | partial | specs cg-*: implemented 2026-09-18, pending verify-pipeline | Run the verify pipeline and record results |
| Walk hardening | partial | spec codegraph-walk-hardening = planned 0%, ledger = stable (contradiction) | Verify against spec and reconcile status |
| Structural fingerprints (`ksm-sfp/1`) | partial | ledger `feat-code-graph-fingerprint` = in_progress | Finish and document format |
| Sidecar reliability | partial | #3590 merged (integrity); issue #3539 closed, #3540 (swallowed errors) open | Fix #3540 |
| Multi-project isolation in the code index | partial | live index stores symbols without a project root; extra roots capped by env (CLI parity audit, ADR-039) | Populate project root, lift root cap, per-project queries |
| `xavier index-self` | implemented | `src/cli/commands/memory.rs`, `src/cli/server.rs`, `src/cli/handlers/memory.rs`, `tests/cli_index_self.rs` in `tests/suite_01.rs` | - |

### MCP surface: 56% (weight 10)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Memory, context and system tools on MCP HTTP (:8100) and stdio | implemented | 59 tools on :8100 (CLI parity audit); `src/server/mcp/` | - |
| Tool parity between :8006 and :8100 | partial | 14 vs 59 tools (CLI parity audit P2-4), in progress | Align surfaces or document the split |
| MCP health honesty | partial | ledger `feat-mcp-health-honesty` = beta | Promote with tests |
| E2E observability contract | partial | ledger beta; flaky health test #3541 | Fix flake, promote |
| MCP contract documentation | implemented | `docs/MCP_CONTRACT.md` | Keep in sync with tool registry |
| `telecom_*` tools | stub | `src/server/mcp/telecom_tools.rs` returns `telecom backend unavailable` | Wire a backend or hide the tools |
| Tools in headless mode | stub | `src/server/headless/routes.rs`: tool "not implemented in headless mode" | Implement or document as unsupported |

### CLI parity: 64% (weight 10)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Command surface (66 top-level, 212 leaf commands) | implemented | CLI parity audit 2026-10-27, `src/cli/commands/` | - |
| `xavier config` get/set/list with `--json` | implemented | #3587 | - |
| Single data-dir resolution (no split-brain) | implemented | #3594 | - |
| Recovery seal safety | implemented | #3588 | - |
| `xavier llm` configuration | implemented | #3591 | - |
| Dead CLI endpoints (provider list/set/auto, export, space, models) | partial | CLI parity P0-4, in progress | Point commands at live routes or remove them |
| Canonical projects list | missing | CLI parity P1-3, in progress | Add `projects` list with `--json` |
| Service management | partial | CLI parity P1-5, in progress | Install/start/stop/status parity |
| `--json` coverage | partial | 23% of leaf commands (audit), sweep P2-1 in progress | Reach 100% of read commands |
| Health / stats parity | partial | ledger `feat-cli-health-stats-parity` = beta | Promote |
| Bounded `doctor` / `improve` | partial | ledger beta | Promote |
| `xavier space lock` | stub | `src/cli/commands/space.rs`: "lock is not implemented on this version" | Implement with spaces runtime |

### LLM and provider configuration: 53% (weight 7)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Provider configuration and routing (local Ollama, cloud providers) | implemented | #3591, `docs/reference/CONFIG_REFERENCE.md` | - |
| Isolated LLM for explain | implemented | explain series #3536-#3562 | - |
| Provider list/set/auto and `/v1/models` routes | partial | dead endpoints in CLI parity audit (P0-4) | Serve real routes |
| API-key proxy vault (provider forwarding) | partial | spec api-key-proxy-vault = in_progress 65%; `handlers/clavis.rs` forwarding not implemented on node | Finish provider forwarding |
| Maloca model router (`/v1/maloca/models/*`) | stub | `src/server/maloca/model_routes.rs` returns 501 | Implement or unmount |
| LLM provider dashboard v2 | missing | spec llm-provider-dashboard-v2 (no status) | Build |

### Config and runtime operations: 75% (weight 8)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Canonical config + env var registry | implemented | #3587, #3589, `docs/reference/env_vars.toml`, `scripts/check-env-vars.py` | - |
| Daemon serves while starting | implemented | #3549 (ledger still `planned`) | Update ledger |
| Sidecar integrity | implemented | #3590 | - |
| SQLite WAL remediation | implemented | `docs/KNOWN_ISSUES.md` WAL section | - |
| Health / readiness truth (`health-truth`, `guardian-truth`) | partial | ledger beta | Promote |
| Live install hygiene (real code-graph binary, runtime data dir, no test users) | partial | CLI parity live findings | Clean up after owner approval |
| Integration suites on `main` | partial | #3554 open, #3586 open PR | Run integration suites on main |
| Flaky health test | partial | #3541 open | Fix flake |

### Vault, recovery, backup and USB: 46% (weight 8)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| `xavier recovery` (passphrase, store, seal) | implemented | `src/recovery/`, `src/cli/handlers/recovery.rs`, #3588 | - |
| Clavis vault (secret lending, MCP secret tools) | partial | ledger `feat-vault-hardening` = beta; leases dashboard not routed | Route leases dashboard, promote |
| USB detection and air-gap capsule | partial | `src/system/usb_detector.rs`, `src/crypto/airgap_capsule.rs`, `docs/ARCHITECTURE/AIRGAP_CAPSULE_PROTOCOL.md` | End-to-end USB backup/restore test and user guide |
| Classified clearance | partial | spec classified-clearance: F3 and F4 pending | Finish F3/F4 |
| Cloud backup | partial | client present; hosted service needs credential renewal | Restore service, add restore drill |
| Master key export/import over HTTP | stub | endpoint returns 501 (`src/cli/handlers/recovery.rs`) | Implement or keep CLI-only and document |
| Local backup command (`xavier backup`) | missing | CLI parity P1-4 | Implement local snapshot + restore |

### Multi-tenant and enterprise: 44% (weight 6)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Enterprise core (tenants, RBAC, audit, rate limits, API keys) | implemented | `src/enterprise/` incl. `tests.rs` | - |
| User administration CLI | partial | `src/cli/commands/users.rs` (enrollment, backup codes) | Complete create/list/disable flows |
| OAuth providers | partial | `src/auth2/` returns 501 for unconfigured providers | Document per-provider setup, add tests |
| Tenants CLI | missing | CLI parity P2-2, in progress | Add `tenants` commands with `--json` |
| Spaces runtime (encrypted spaces) | stub | `src/cli/http_setup.rs` returns 501 `EncryptionPending`; ledger `espacio-runtime` planned | Implement spaces runtime |

### Agent onboarding, skills and sessions: 70% (weight 6)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Installer / onboarding (`xavier init`, agent setup) | implemented | #3593, `src/cli/commands/installer.rs` | - |
| Canonical agent skills | implemented | README 'Canonical Agent Skills' | - |
| Sessions | implemented | `src/cli/commands/session.rs` | - |
| Skill dispatch confidence floor | partial | ledger beta | Promote |
| Agent management in headless mode | stub | `src/cli/handlers/headless_api.rs` returns not implemented | Implement or document |
| Skill subsystem redesign (store, registry, projector, ephemeral, drift audit, telemetry) | partial | code in `src/context/skill_controller/` (projector, telemetry, audit, policy); ledger `feat-skill-*` still `planned` | Finish store/registry/ephemeral, verify, update ledger |

### Cloud, SaaS and billing: 43% (weight 6)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Plan catalog v1 | implemented | `src/billing/plans.rs` (vendored catalog) | - |
| `xavier billing` client (catalog, checkout, usage) | implemented | `src/billing/client.rs`, `src/cli/handlers/billing.rs` | - |
| Hosted billing endpoint wiring | partial | default billing URL is local (`127.0.0.1:8787`); production set by env | Ship a production default and a status check |
| Hosted cloud usage / sync | partial | `src/memory/cloud_sync.rs`; hosted endpoint responds | End-to-end usage + sync test |
| Cloudflare Vectorize backend | missing | spec 070-cloudflare-vectorize: approved, pricing pending | Pricing decision, then implement |
| Headless server packaging | missing | ledger planned, spec without status | Package and document |
| Marketplace on-chain settlement | missing | `crates/xavier-market/src/config.rs`: not implemented in v1 | Out of v1 scope; decide for v2 |

### Mesh, P2P, IVN and Maloca: 29% (weight 6)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Mesh P2P sync (iroh, optional build feature) | partial | ledger beta; `Cargo.toml` feature `mesh` | Default-on decision, e2e tests (#3195) |
| Service network telemetry | partial | ledger beta | Promote |
| Private mesh per key wallet | partial | ledger beta | Promote |
| Mesh maturity in `/health` | partial | `src/mesh/maturity.rs` starts unmeasured | Measure real signals |
| libp2p transport | stub | empty feature, noted as deprecated stub | Remove or implement |
| Tokenomics / wallet | stub | `src/mesh/tokenomics/mod.rs` placeholder | Design + implement |
| Governance / DAO | stub | off-chain store with fixed example entries; on-chain behind `dao-evm` | Real proposal store |
| Maloca alignment routes | stub | static checklist (`src/server/maloca/alignment_route.rs`) | Compute real alignment |
| gpud router (`/v1/gpud/*`) | stub | defined in `src/server/gpud_sidecar.rs`, never mounted | Mount or remove |
| IVN, IVN API, IVN karma | missing | specs planned 0% (ledger says stable - contradiction) | Implement or correct ledger |
| Mesh replay store | partial | code in `src/mesh/replay_store.rs`; ledger still `planned`, spec without status | Verify against spec, add e2e test, update ledger |
| Maloca roadmap / governance rules / atomic store persist | missing | ledger planned | Implement |

### Panel, desktop and packaging: 70% (weight 4)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Browser panel | implemented | README UI modes; KNOWN_ISSUES #1-#6 fixed | - |
| Tauri desktop | implemented | README UI modes | - |
| Docker deployment | implemented | `docs/DEPLOY/DOCKER_DEPLOY.md` | - |
| Android build | partial | #3572 open | Land NDK build |
| Generative UI (OpenUI) | missing | ledger planned, spec 0% | Implement |

### Docs, CI and release: 67% (weight 4)

| Feature | Status | Evidence | Remaining to 100% |
|---|---|---|---|
| Required PR CI checks | implemented | branch ruleset, `docs/ops/main-ruleset.md` | - |
| Env var documentation | implemented | #3589 | - |
| Release pipeline | partial | #2666, #2665, #2639 open | Close release issues |
| Status docs accuracy | partial | `docs/FEATURE_STATUS.md` and `implementation-score.json` claim 100% | Use this document as the measured baseline |
| English API docs | implemented | `docs/api/README.md` is English, KNOWN_ISSUES #14 resolved | - |
| Autonomous delivery pipeline | missing | ledger planned, spec 0% | Implement |

## Contradictions between sources

- `docs/FEATURE_STATUS.md` reports "52/52 stable 100% verified" and `implementation-score.json` reports `overall_pct: 100`, while the ledger itself lists 25 planned, 16 beta and 2 in-progress features.
- Explain features (`explain-*`, `code-explain-context`, `fix-explain-repo-resolution`) are `planned` in the ledger but merged (#3536, #3537, #3545, #3561, #3562).
- `fix-daemon-serve-while-starting` is `planned` in the ledger but merged (#3549).
- IVN, IVN API and IVN karma are `stable` in the ledger while their specs are `planned` at 0%.
- `codegraph-walk-hardening` is `stable` in the ledger while its spec is `planned` at 0%.
- The mesh replay store and parts of the skill subsystem have code on `main` (`src/mesh/replay_store.rs`, `src/context/skill_controller/`) while the ledger lists them as `planned`.
- The code-graph O1-O7 specs say "implemented" but are still pending the verify pipeline.

## Top gaps to 100%, in priority order

1. **Finish CLI parity** (in progress): dead endpoints, projects list, service management, `--json` sweep, tenants CLI, MCP surface parity.
2. **Backup and recovery**: local `xavier backup` and restore, a decision on master key export/import, the cloud backup service restored, and an end-to-end USB backup/restore test.
3. **Code-graph in production**: multi-language, multi-project reindex with a freshness signal, fixes for #3540, and the O1-O7 verify pipeline.
4. **CI truth**: integration suites on `main` (#3554, #3586) and the flaky health test (#3541).
5. **Promote beta features** with evidence: encryption at rest, memory write integrity, health truth, MCP health honesty, vault hardening, per-repo memory.
6. **Spaces runtime and multi-tenant CLI**: spaces encryption, `space lock`, tenants and user admin flows.
7. **LLM routes**: provider list/set/auto and `/v1/models`, the Maloca model router, provider forwarding in the API-key proxy, dashboard v2.
8. **Cloud / billing wiring**: a production billing default, an end-to-end usage and sync test, the Vectorize pricing decision, headless server packaging.
9. **Skill subsystem redesign**: finish and verify the six ledger features, plus headless agent management.
10. **Mesh / IVN / Maloca**: decide the v1 scope, then implement or demote the stubs (libp2p, tokenomics, DAO, gpud, alignment) and IVN.
11. **Release and docs**: release pipeline issues, generative UI, autonomous delivery.
12. **Status hygiene**: reconcile `.gitcore/features.json`, `FEATURE_STATUS.md` and `implementation-score.json` with this document.
