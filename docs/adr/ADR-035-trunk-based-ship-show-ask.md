# ADR-035 — Protected trunk-based integration with Ship / Show / Ask

| Field | Value |
| --- | --- |
| **ID** | ADR-035 |
| **Estado** | Propuesto |
| **Date** | 2026-09-27 |
| **Authors** | Xavier maintainers |
| **Related** | `docs/design/trunk-based/00-ANALYSIS.md`; `docs/design/trunk-based/01-DESIGN.md`; `AGENTS.md:44-49`, `AGENTS.md:80-90` |

## Context

Xavier's stated process uses waves, a feature ledger and one PR per feature (`AGENTS.md:25-27`, `AGENTS.md:38-55`, `AGENTS.md:80-84`). Recent CI wall time is p50 9.4 minutes, with root Rust tests at 9.7 minutes median when run (`facts/FACTS.md:4-5`). `main` currently has no required status checks or review rule (`facts/FACTS.md:2`), while most contributors are agents with observed scope and evidence errors (`facts/FACTS.md:7-8`). The current CI has no ledger verifier invocation, despite the written PR rule (`AGENTS.md:44-49`; `.github/workflows/ci.yml:14-292`). The owner's proposal seeks faster integration through a short gate, post-merge verification and Ship/Show/Ask review timing. Binary deployment is local, not continuous; release is tag-driven (`facts/FACTS.md:9`; `.github/workflows/release.yml:3-8`).

## Competing options

| Option | Description | Expected cost / complexity |
| --- | --- | --- |
| **A — Keep current PR flow** | Continue current CI and informal review, repair the missing ledger invocation, and add branch protection without test impact selection. | Least policy change; measured PR latency remains around current CI time and direct pushes still need restriction. |
| **B — Protected trunk with PR Ship/Show/Ask** | Required always-run fast gate, scoped PR checks, auto-merge for low-risk Ship/Show PRs, independent agent-panel approval for Ask, full async `main` verification and tracked failure response. Agents never push directly to `main`. | CI/path-classification work, ruleset administration, and possible post-merge reverts; smaller expected wait if impact selection proves sound. |
| **C — Literal proposal, direct Ship/Show pushes** | Let authors push Ship/Show directly to `main`, run only local hooks before push, and rely on async CI for remaining failures. | Fastest nominal integration but no server-side check before an agent push under present rules; highest recovery burden and conflicts with one-PR-per-feature policy. |

## Simulation (REQUIRED)

**PENDIENTE.** The independent agent panel must not accept this ADR until the multi-scenario simulation is run, deterministic gates pass, two independent models approve, and the auditable report is stored. No winner, score, margin or sensitivity result is asserted here.

- **Engine:** `swal-sim/adr_sim.py`
- **Exact command** (from the simulator directory): `python3 adr_sim.py --model trunk_based_flow --runs 5000 --seed 42 --outdir reports`
- **Runs / seed:** 5000 / 42, requested; not executed.
- **Result:** PENDIENTE (composite winner and primary-only winner).
- **Agreement and sensitivity:** PENDIENTE (including bull/base/bear/worst removal).
- **Report:** PENDIENTE (record generated Markdown and JSON paths at acceptance).

## Assumptions and anchors

| Parameter | Value used | Source |
| --- | --- | --- |
| Existing CI wall time | p50 9.4 min, p90 11.1 min across 30 recent runs | `facts/FACTS.md:4` |
| Root tests / Clippy time | Median 9.7 min / 3.2 min on Rust runs | `facts/FACTS.md:5` |
| Direct-push protection | No required check/review on `main` | `facts/FACTS.md:2` |
| Agent share and error modes | Mostly agents; scope/path/evidence mistakes observed | `facts/FACTS.md:7-8` |
| Fast gate under 3 min | Target, **ASSUMPTION** pending shadow measurement | `docs/design/trunk-based/00-ANALYSIS.md` |
| Test impact sensitivity | **ASSUMPTION**; selected tests might miss cross-layer regressions | `.github/workflows/ci.yml:198-204` |
| Binary rollback | **ASSUMPTION** that a Git revert does not change an installed local binary | `facts/FACTS.md:9`; `.github/workflows/release.yml:3-8` |

## Decision

**Proposed option B, subject to simulation and Phase 1/2 evidence.** Keep the feature ledger and bounded PR convention, use Ship/Show/Ask for review timing, require a server-side gate before agent merges, and run full verification after merge with incident tracking. Do not permit routine direct agent pushes. The independent agent panel accepts this ADR only after deterministic gates, the required simulation, two independent model approvals, and an auditable report tied to the revision. Until that acceptance, existing PR verification obligations remain in force (`AGENTS.md:44-49`, `AGENTS.md:80-84`); any later change distinguishing structural PR ledger checks from full post-merge verification needs a separate policy edit. A measured fast-gate miss or impact-selection miss delays protection changes rather than being waived to meet the schedule. No winning margin is claimed before simulation.

## Consequences

- **Positive:** consistent server-side evidence for agent changes, short no-review path for bounded low-risk work, explicit visibility of red `main` runs.
- **Negative / cost:** impact-map maintenance, CI duplication during rollout, agent-panel review and incident response, and temporary regressions on `main` if async checks find a miss.
- **Invalidation:** selected PR checks miss failures found by full checks; `main` incidents exceed the change-failure target; median recovery exceeds target; or fast-gate p90 stays above target without a safe way to split tests. In those cases restore the prior required check set and reassess the option.

## Follow-up verification

After Phase 3, review four weekly cohorts of GitHub Actions and PR/issue data using the definitions in `docs/design/trunk-based/00-ANALYSIS.md`. Confirm fast-gate p50/p90, lead time, MTTR, change-failure rate, direct-push rejection and incident closure. Record the simulator report and agent-panel decision in this ADR; only then change Estado to Aceptado. The owner reviews the resulting product through the Product Acceptance Package.
