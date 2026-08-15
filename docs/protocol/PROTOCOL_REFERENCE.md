# Protocol Reference — Wave-Based Development

**Scope:** public engineering protocol for Xavier.
**Source of truth:** `docs/features/features.json` (the feature ledger).

## Pipeline

```
PHASE 0: PRE-WAVE
  ├── Web research (hallazgos técnicos, fuentes, referencias)
  ├── Verify features (scripts/verify-pipeline.sh)
  │     → Real state of the ledger (not the claimed one)
  └── Pre-wave validation (endpoints, assumptions, blockers)

PHASE 1: WAVE DESIGN
  ├── Decompose into N issues (recommended: 15 max)
  ├── Dependency tree (max 3 levels deep)
  ├── File islands: parallel issues touch DISJOINT files
  └── Agent assignment per issue

PHASE 2: ISSUE GENERATION
  ├── Local issues under .gitcore/issues/ (or project tracker)
  ├── Each issue maps to features.json entries
  └── Dependency tree documented

PHASE 3: EXECUTION
  ├── Agents work in isolated worktrees / sandboxes
  └── Sequential merge after execution

PHASE 4: VERIFICATION
  ├── scripts/verify-pipeline.sh — 0 failures
  ├── cargo check/clippy/test — green
  └── features.json updated (status promoted only by green runs)
```

## Feature Ledger Contract

`docs/features/features.json` is the single source of truth.

```json
{
  "version": "3.8.0",
  "project": "xavier",
  "features": [
    {
      "id": "feat-example",
      "title": "Example feature",
      "progress_pct": 100,
      "status": "stable",
      "tests": ["cargo test -p xavier --lib example"],
      "implemented_in": ["src/example.rs"],
      "last_tested": "2026-08-01"
    }
  ]
}
```

- **status:** `planned` → `beta` → `stable`. Promotion requires a green verify run.
- **tests[]:** commands that prove the feature works. The pipeline executes them.
- **implemented_in[]:** files that implement the feature (path-existence is checked).
- **progress_pct:** honest percentage, reconciled by the pipeline, not by hand.

## Wave Rules

- Max 3 dependency levels from the root issue.
- Issues at the same level with no dependencies → parallelizable.
- File islands: parallel issues MUST NOT share files (verify before dispatch).
- A wave closes only when ALL its features are `stable` (green runs).

## Verification Is Public

Anyone can reproduce the state of the project:

```bash
scripts/verify-pipeline.sh          # ledger validation + test execution
```

The pipeline must be able to run on a fresh clone with no personal
configuration. All environment-dependent paths come from env vars with generic
defaults (see `.env.example`).

## Security Boundary

- No credentials, tokens, or personal paths live in this repo.
- Everything environment-specific goes to `.env` (ignored) — see `.env.example`.
- Secrets are checked by `scripts/check-secrets.sh` (gitleaks) before merge.
