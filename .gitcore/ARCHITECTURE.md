# Architecture decisions (GitCore)

Non-negotiable decisions for this repository under GitCore 3.8.0. Product
architecture lives in `docs/SRS/ARCHITECTURE.md` and ADRs in `docs/adr/`.

## Ledger

- `.gitcore/features.json` is the single feature ledger. Other status documents
  (`docs/FEATURE_STATUS.md`, `docs/features/implementation-score.json`) are derived
  from it or are dated snapshots.
- Feature status changes only through the Ledger Promoter
  (`scripts/ledger-promote.py`, `.github/workflows/ledger-promote.yml`) on same-SHA
  `main` evidence, and lands through a pull request that passes the normal checks.
  Status is never raised or lowered by hand.

## Exception: GitHub Actions enabled

GitCore 3.8 disables workflows by default (`.github/workflows.disabled/`). This
repository keeps them active in `.github/workflows/`.

- **Why:** remote agents never build locally (`AGENTS.md` section 2), so CI is the
  only place where the workspace is compiled and tested. The ledger verifier and the
  Ledger Promoter also need `main` CI evidence.
- **Conditions:**
  - Required checks on pull requests stay limited to the merge gates (CI Gate and
    Shadow Fast Gate).
  - The Ledger Promoter runs only on manual dispatch, only when the repository
    variable `LEDGER_PROMOTER_ENABLED` is `true`, defaults to a dry run, and opens a
    pull request instead of pushing to `main`.
  - Workflows must not need personal configuration; secrets come from repository
    secrets only.
- **Review:** revisit this exception if CI cost or noise becomes a problem, or if the
  repository moves to local-only CI (`docs/protocol/LOCAL_CI.md`).
