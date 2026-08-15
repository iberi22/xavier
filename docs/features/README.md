# Feature Ledger — `features.json`

The feature ledger is the **source of truth** for what Xavier does and how it is
verified. It lives at `docs/features/features.json`.

## Status semantics

| Status | Meaning |
|--------|---------|
| `planned` | Spec exists; no implementation yet |
| `beta` | Implemented; not all tests green / unstable |
| `stable` | All `tests[]` pass on a clean verify run |

> **A feature is `stable` only when the pipeline says so.** The `status` field
> is never hand-edited to a higher state without a green run.

## Fields

| Field | Purpose |
|-------|---------|
| `id` | Unique feature id (`feat-*` or `F-XXX`) |
| `title` | Human-readable name |
| `progress_pct` | Honest percentage (reconciled by the pipeline) |
| `status` | `planned` / `beta` / `stable` |
| `tests[]` | Commands proving the feature (executed by the pipeline) |
| `implemented_in[]` | Source files implementing it (existence-checked) |
| `last_tested` | Date of the last green run |

## Verify it yourself

```bash
# From the repo root:
scripts/verify-pipeline.sh

# The pipeline:
#   1. preflight: checks required tools (python3, cargo, git)
#   2. structure: validates the ledger schema
#   3. existence: every implemented_in[] path must exist
#   4. tests:     runs tests[] of stable+beta features
#   5. score:     prints the real implementation percentage
# Exit code 0 = all green. Any failure = the ledger is lying.
```

## Adding a feature

1. Write the spec: `docs/features/specs/FEATURE-<id>.md`
   (acceptance criteria, files, tests).
2. Add the ledger entry (`status: "planned"`, `progress_pct: 0`).
3. Implement in a PR referencing the feature id.
4. Let the pipeline promote it — never self-promote.

## `implementation-score.json`

`docs/features/implementation-score.json` is a **release snapshot** of the
aggregate score, regenerated at release time by the pipeline. It is an output
artifact, never hand-edited. See `docs/protocol/SDLC_WORKFLOW.md` §5.
