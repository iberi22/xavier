# SDLC Workflow

The full lifecycle of a change in Xavier, from idea to shipped feature.

## Loop

```
SPEC  →  PLAN  →  BUILD  →  VERIFY  →  SHIP
 │         │        │         │         │
 ADR      wave     agents    pipeline   changelog
 design    issues   (work-    (green)    + tag
          tree      trees)
```

## 1. SPEC

- Every non-obvious decision becomes an ADR in `docs/adr/`
  (context → decision → consequences). Debates happen in public issues/PRs.
- New features: write the spec first (`docs/features/specs/FEATURE-*.md`)
  with acceptance criteria, then add the entry to `features.json`.

## 2. PLAN

- Design the wave: dependency tree (≤3 levels), file islands, agent assignment.
- Each issue references its feature id (`F-XXX` / `feat-*`).

## 3. BUILD

- 1 PR = 1 feature (or a bounded part of it).
- Agents and humans follow the same rules:
  - `cargo fmt` + clippy clean (warnings are errors in CI).
  - Tests are written with the feature (RED-GREEN-REFACTOR).
  - Never touch files outside the issue's file island.

## 4. VERIFY

- `scripts/verify-pipeline.sh` executes the `tests[]` of every feature.
- A feature is `stable` only after its tests pass on a clean run.
- `last_tested` is updated by the run, not by hand.

## 5. SHIP

- Merge → `CHANGELOG.md` entry → tag → release.
- `docs/features/implementation-score.json` is regenerated as a release snapshot
  (never edited by hand; the pipeline derives it from the ledger).

## Rules of the Road

1. **Never** change a feature's `status` to `stable` by hand without the
   pipeline run that proves it.
2. **Never** commit output artifacts (reports, scores) — they are generated.
3. **Never** commit session state (handoffs, closeouts, agent logs).
4. **One canonical JSON** — `features.json`; everything else is derived.
