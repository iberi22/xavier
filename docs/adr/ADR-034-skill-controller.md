# ADR-034 — Canonical Skill Sources and Declarative Tool Views

| Campo | Valor |
|-------|-------|
| **ID** | ADR-034 |
| **Estado** | Propuesto |
| **Fecha** | 2026-09-27 |
| **Autores** | Xavier architecture contributors |
| **Relacionados** | ADR-016, ADR-019 (narrow amendment proposed), REQ-060..064, Skill Controller brief |

## Contexto

Xavier requires selective CLI placement, task-scoped context assembly, and drift visibility across multiple agent harnesses while remaining file-based and local-first (`.gitcore/docs/SWAL_GOAL.md:8`, `docs/design/skill-controller/00-BRIEF.md:58`). Currently, Hermes canonical discovery is hardcoded directly into the skill registry (`src/context/skill_registry.rs:105`). Moving source authority away from a single tool's discovery path must preserve existing offline file access, respect source ownership, and avoid publishing the complete catalog to all tools.

Adopting an in-process controller in the `xavier` application crate requires addressing ADR-019's blanket statement that everything outside the storage core leaves as a plugin or sidecar. Its storage-core membership rule and C1/C2/C3 sidecar boundaries remain binding.

### Enmienda a ADR-019

ADR-019 (`docs/adr/ADR-019-plugin-first-boundary.md:54-57`, `:90-98`) restricts the storage core to database, embeddings, and storage protocol responsibilities and states that everything else leaves as an external plugin or sidecar. This **proposed amendment** makes one explicit exception to that blanket placement sentence: the Skill Controller may be an in-process module of the `xavier` **application crate** under `src/context/skill_controller/` (containing `manifest`, `policy`, `projector`, `composer`, `audit`, and `telemetry`; D6). It does not classify the controller as storage core, relax ADR-019's core membership rule, or change the C1 CodeGraph, C2 Mesh, or C3 Panel UI boundaries. Acceptance of this amendment remains a human decision (P03).

The proposed exception is conditional on these invariants:
1. **Core Logic Isolation:** Nothing is added to `crates/xavier-core-logic` (core vector store and embedding calculation primitives remain untainted).
2. **No New Daemon or Plugin System:** No new background daemons, network services, DSLs, or plugin frameworks are introduced (D8).
3. **Preserved Sidecar Boundaries:** The C CodeGraph AST parser remains an external UDS sidecar (`src/codebase/codegraph_client.rs:108`), and the SWAL Mesh P2P network remains strictly decoupled.
4. **Storage Reuse:** Operational state, journals, and telemetry reuse existing SQLite migration infrastructure (`src/storage/mod.rs:26`) and the resolved data directory (`src/settings/mod.rs:123`, ADR-016); no secondary database is created.

## Opciones que compiten

| Opción | Descripción | Coste/complejidad esperada |
|--------|-------------|----------------------------|
| A | Git repository directly at `$HOME/.agents/skills`; per-skill views elsewhere. | Moderate migration; lowest Codex integration cost; exposes full source catalog globally to Codex and OpenCode. |
| B | Retain `$HOME/.hermes/skills` as canonical; add versioning and projection. | Lowest immediate change; Hermes discovery remains tightly coupled to the global catalog. |
| C | Dedicated non-discovered Git source outside CLI discovery roots; per-skill projections into all harness roots. | Highest one-time migration of these three; complete separation of authorship and discovery. |

> Rejected publication alternatives: Linking entire discovery directories is rejected because it prevents per-tool selection and displaces tool-owned entries (D2). Broad file copies are rejected except as an explicit per-adapter fallback for filesystems failing symlink discovery (D2).

## Simulación (OBLIGATORIO)

> Ninguna decisión se acepta sin simulación multi-escenario. Ver skill `swal-adr-simulation`.

The `skill_controller_store` model compares A/B/C across bull, base, bear, and worst scenarios. Its outputs are simulated estimates, not field measurements; every input without a cited anchor is an **ASSUMPTION** (see below and the report).

Reproduction command:

```sh
python3 -B "$HOME/proyectosSWAL/periferia/swal-sim/adr_sim.py" \
  --model skill_controller_store --runs 5000 --seed 42 \
  --outdir "$HOME/proyectosSWAL/periferia/swal-sim/reports"
```

- **Motor:** `$HOME/proyectosSWAL/periferia/swal-sim/adr_sim.py`, model `skill_controller_store` (iberi22/swal-sim, branch `feat/adr-034-skill-controller-model`, commit `b45ba5d`).
- **Runs / seed:** 5000 / 42.
- **Resultado:** composite winner **C** (0.800), followed by B (0.451) and A (0.093). Primary metric `unwanted_discovery_rate` (lower is better): **C** 0.01604, B 0.5614, A 0.8616. Primary-only winner: **C**.
- **¿Coinciden?** Sí: composite and primary-only winners are both C.
- **Trade-off:** C has the **highest modeled migration effort**: 23.97 hours versus A 16.51 and B 7.888 hours. These hours are **ASSUMPTION-based estimates**, not a completed migration or measured rollout cost.
- **Sensitivity:** dropping bull, base, bear, or worst separately leaves **C** as winner in every case. This is stability under the report's scenarios and **ASSUMPTION-based inputs**, not proof of CLI compatibility.
- **Reporte:** `$HOME/proyectosSWAL/periferia/swal-sim/reports/ADR-034-skill_controller_store.md` and `.json`.

## Supuestos y su anclaje

| Parámetro | Valor usado | Fuente |
|-----------|-------------|--------|
| Existing canonical scan root | Hermes plus workspace `skills/` and `.agents/skills` | `src/context/skill_registry.rs:105` |
| Native global discovery | Codex reads `.agents`; OpenCode also reads `.claude` | `docs/design/skill-controller/00-BRIEF.md:34` |
| Canonical catalog inside discovery root defeats selective exposure | Architectural inference for Options A/B | `docs/design/skill-controller/01-ARCHITECTURE-AND-ADR.md:130-134` |
| File operation without Xavier | Required | `docs/design/skill-controller/00-BRIEF.md:11` |
| Per-skill links supported by every target CLI | ASSUMPTION, adapter acceptance gate | `docs/design/skill-controller/01-ARCHITECTURE-AND-ADR.md:255` |
| Scenario sizes, weights, conflict and store-loss rates; objective weights | ASSUMPTION: model inputs, not field observations | `skill_controller_store` in `$HOME/proyectosSWAL/periferia/swal-sim/adr_sim.py` |
| Migration effort, recovery time, drift rates, and unwanted-discovery baseline values | ASSUMPTION: model inputs; reported outputs are simulated estimates | `skill_controller_store` in `$HOME/proyectosSWAL/periferia/swal-sim/adr_sim.py` |
| Ephemeral token budget / stale lease / telemetry retention | 4000 tokens / 86400 seconds / 30 days | ASSUMPTION: initial configurable policy (D3, D11, D12) |
| Relative migration costs in options table | Qualitative estimates | ASSUMPTION |

## Decisión

Propose **Option C with per-skill symlinks** across harness discovery roots, governed by reconciled decisions D1–D14. The `skill_controller_store` simulation favors C on both composite and primary metric, subject to its **ASSUMPTION-based** inputs and adapter rollout tests. This ADR and the ADR-019 amendment remain **Propuesto** until human acceptance (P03).

1. **Canonical Store (D1):** Dedicated Git source outside CLI discovery roots, configured by `XAVIER_SKILL_STORE` (example `$HOME/.local/share/xavier/skill-store`). Hermes remains authoritative until an approved migration completes.
2. **Publication Mechanism (D2, D9):** One symlink per selected skill directory; whole-directory symlinks are prohibited. Swaps are atomic: create `symlink(src, dir/.tmp-<rand>)`, `rename` over destination, and `fsync` the parent directory. Managed copies serve only as measured fallbacks.
3. **Manifest Format (D3):** Versioned TOML v1 schema parsed using the existing `toml` crate (`Cargo.toml:262`). Explicit placements only; no globs or implied install-all.
4. **Mutation Surface (D4):** CLI-only (`xavier skills plan|apply|rollback|compose|release|audit|usage`). REST and MCP interfaces remain strictly read-only (drift reports, usage summaries, dispatch). `plan` is the default and writes nothing; `apply --plan <file>` rechecks preconditions.
5. **Environment Configuration (D5):** 12-factor variables documented in `.env.example`: `XAVIER_SKILL_STORE`, `XAVIER_SKILL_MANIFEST`, `XAVIER_SKILL_SOURCE_ROOTS`, `XAVIER_SKILL_TARGET_ROOTS`. Roots are host-authorized allowlists that repository content cannot widen.
6. **Code Location (D6):** New module `src/context/skill_controller/` in the `xavier` application crate; zero additions to `crates/xavier-core-logic`.
7. **Prerequisite Refactoring (D7):** Before projector implementation: (a) configurable store/config paths with backward-compatible defaults; (b) single `build_skill_registry(workspace)` constructor used by REST, MCP, and session fusion; (c) named constants for dispatch confidence (`0.40`, `src/context/skill_dispatcher.rs:79`) and fusion confidence (`0.50`, `src/server/http/context.rs:15`).
8. **Dependencies (D8):** Reuse existing crates (`toml`, `walkdir`, `regex`, `aho-corasick`, `sha2`, `pulldown-cmark`); promote `tempfile` to `[dependencies]`; add `similar` only for drift proposal diffs. No crate-wide YAML replacement.
9. **Secret Scanning (D10):** Native `RegexSet` scanner over entire packages and composed outputs, mirroring `scripts/check-secrets.sh`. Findings report location and pattern class only; publication is blocked on any finding.
10. **Deterministic Ephemeral Composition (D11):** Ephemeral skills are deterministic (no LLM), bounded by `ephemeral.max_tokens`, provenance-stamped, and scoped by authorized workspace. The caller's `project` string is not authorization; memory and decision queries must filter by authorized workspace scope (`src/context/skill_dispatcher.rs:207`).
11. **Telemetry (D12):** Local SQLite events via the existing migration framework (`src/storage/mod.rs:26`), opaque IDs, no prompt text or secrets, tracking stages `selected/delivered/invoked/completed`, with 30-day retention; never used for automated deletion.
12. **Drift Audit (D13):** Report-only; offline by default; generates proposed diffs for human review; never auto-edits secrets, safety rules, or approval rules.
13. **Governance (D14):** Rules block and DoD capped at 4 files / 400 changed lines per implementation issue.

## Consecuencias

- **Positivas:**
  - Deliberate, per-tool skill exposure without global catalog pollution.
  - Full Git provenance and auditability for canonical skill authoring.
  - Complete offline operability: tools read standard files even if Xavier is stopped.
  - Reversible, atomic projection with write-ahead journals and rollback.
  - Clean separation between storage primitives (ADR-019) and application orchestration.
- **Negativas / coste:**
  - Highest modeled one-time migration effort (23.97 hours, **ASSUMPTION-based**) and prerequisite backup procedures; Hermes remains authoritative until approved migration completes.
  - Adapter maintenance to verify symlink discovery across CLI updates.
  - Dependency on source directory availability for symlink resolution.
  - Incomplete telemetry visibility for direct filesystem reads without harness hooks.
- **Qué invalidaría esta decisión:**
  - Field measurements or revised `skill_controller_store` assumptions showing Option A or B superior across multi-scenario runs.
  - Native per-tool discovery filters implemented upstream in CLIs, making separate projection redundant.
  - Discovery failure of per-skill symlinks across core CLI harnesses (Codex, Claude Code, OpenCode).
  - Migration or operational overhead exceeding measured benefits.

## Verificación posterior

Before rollout and for human acceptance (P03):
1. Review the completed `skill_controller_store` simulation, its **ASSUMPTION-based** inputs, and the stable composite/primary winner C.
2. Verify startup discovery for each supported CLI harness with Xavier stopped; CLI compatibility is not yet established.
3. Validate atomic symlink swapping, crash recovery, and unmanaged file collision rejection in isolated test fixtures.
4. Verify that REQ-060 and `AGENTS.md` path documentation are reconciled.

Review scheduled for **2026-10-27**, tracking visible skill precision, drift incidence, rollback integrity, and invocation observation coverage. If pilot validation is incomplete by that date, ADR-034 remains **Propuesto**.
