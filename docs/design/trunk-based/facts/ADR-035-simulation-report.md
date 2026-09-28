# ADR-035 — Simulation Report

> Generated: 2026-09-28T03:54:32.154880+00:00 · model `trunk_based_flow` · runs=5000 · seed=42
> Reproduce with: `python3 adr_sim.py --model trunk_based_flow --runs 5000 --seed 42`

**Question:** how should an agent-heavy repo integrate changes into main?

**Primary metric:** `change_failure_rate` (min = better, 0-1)

## Ranking (multi-objective composite, best first)

Objective weights: `change_failure_rate`×0.35, `lead_time_hours`×0.2, `mttr_hours`×0.15, `escaped_defect_rate`×0.15, `owner_review_minutes_week`×0.15

| # | Option | composite | lead_time_hours | change_failure_rate | mttr_hours | escaped_defect_rate | owner_review_minutes_week |
|---|--------|-----------|---|---|---|---|---|
| 1 | B (tbd_panel): the ADR-035 design — required fail-closed fast gate (<3 min target), async full post-merge lane, path-computed Ship/Show/Ask, Ask approved by a two-model agent panel, no agent direct pushes, independent rollback guardian ⭐ | **1.000** | 0.9139 | 0.07477 | 1.304 | 0.03078 | 38.48 |
| 2 | A (current): PRs with full CI (~9.4 min p50) and no required checks/reviews on main (anyone can push) | **0.433** | 9.021 | 0.2485 | 7.131 | 0.1321 | 498.2 |
| 3 | C (direct_push): agents push Ship/Show directly to main with local hooks only | **0.163** | 2.4 | 0.5735 | 9.641 | 0.2501 | 1017 |

**Winner (composite): `tbd_panel`** · primary-only winner: `tbd_panel`

## Ranking (weighted across scenarios)

| # | Option | lead_time_hours | change_failure_rate | mttr_hours | escaped_defect_rate | owner_review_minutes_week |
|---|--------|---|---|---|---|---|
| 1 | B (tbd_panel): the ADR-035 design — required fail-closed fast gate (<3 min target), async full post-merge lane, path-computed Ship/Show/Ask, Ask approved by a two-model agent panel, no agent direct pushes, independent rollback guardian ⭐ | 0.9139 | 0.07477 | 1.304 | 0.03078 | 38.48 |
| 2 | A (current): PRs with full CI (~9.4 min p50) and no required checks/reviews on main (anyone can push) | 9.021 | 0.2485 | 7.131 | 0.1321 | 498.2 |
| 3 | C (direct_push): agents push Ship/Show directly to main with local hooks only | 2.4 | 0.5735 | 9.641 | 0.2501 | 1017 |

**Winner on `change_failure_rate`: `tbd_panel`**

## Per-scenario detail (mean / p10 / p90)

| Option | bull | base | bear | worst |
|--------|---|---|---|---|
| tbd_panel | 0.067 [0.057–0.077] | 0.06456 [0.0545–0.0746] | 0.08839 [0.0784–0.0983] | 0.08251 [0.0725–0.0926] |
| current | 0.2188 [0.198–0.239] | 0.2101 [0.19–0.23] | 0.3 [0.28–0.32] | 0.2776 [0.258–0.298] |
| direct_push | 0.5191 [0.491–0.548] | 0.4928 [0.465–0.52] | 0.665 [0.637–0.693] | 0.66 [0.632–0.688] |

## Sensitivity

- Dropping scenario `bull`: winner would be `tbd_panel` → ✅ winner stable
- Dropping scenario `base`: winner would be `tbd_panel` → ✅ winner stable
- Dropping scenario `bear`: winner would be `tbd_panel` → ✅ winner stable
- Dropping scenario `worst`: winner would be `tbd_panel` → ✅ winner stable

## Referenced parameters (not invented)

- docs/design/trunk-based/facts/FACTS.md:4 (ci.yml last 30 runs p50 9.4 min, p90 11.1 min)
- docs/design/trunk-based/facts/FACTS.md:5 (rust-test median 9.7 min, rust-clippy 3.2 min, n=10 runs)
- docs/design/trunk-based/facts/FACTS.md:2 (no required status checks, no required reviews on main)
- docs/design/trunk-based/facts/FACTS.md:8 (agent failure modes: scope errors, personal paths, unverified test claims)
- docs/design/trunk-based/facts/FACTS.md:6 (Husky 9 pre-commit runs workspace fmt plus package-scoped clippy (-p xavier -p code-graph) and secret scan)
- docs/design/trunk-based/00-ANALYSIS.md:49 (fast-gate target p50 < 3 min, p90 < 3 min for eligible PRs)
- docs/design/trunk-based/00-ANALYSIS.md:51-52 (DORA targets: lead time <1d, MTTR <4h, change_failure_rate <10%)
- docs/design/trunk-based/01-DESIGN.md:31-33 (Ship/Show/Ask classification, 2-model panel for Ask)
- docs/design/trunk-based/07-ROLLBACK-GUARDIAN.md (automated incident creation and revert PR)
- ASSUMPTION: lead time and MTTR distributions modeled from CI wall times and review queues
- ASSUMPTION: local hook bypass rate (15-40%) when agents lack local environment or push via cloud
- ASSUMPTION: scoped fast gate misses 2-5% of cross-layer defects, caught by async post-merge lane
- ASSUMPTION: owner review burden estimated from PR volume and incident triage overhead

> Every value not anchored above is an ASSUMPTION encoded in `MODEL_DEFS`; challenge it before citing this report.
