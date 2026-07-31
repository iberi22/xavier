> Parent EPIC: {{EPIC}}

# [Ola 11 · 11] Docs: local CI when GitHub Actions budget exhausted

> Wave **Ola 11 — Harden Residuals**. Ops truth (docs only).

## Current State (MEDIBLE)
- GHA minutes exhausted blocked Ola 10 checks; merges used admin after diff review
- Host path: `swal-ci-container` + `agent-privilege-notify` for Docker elevation under NoNewPrivs

## Desired State (DELTA)
- NEW `docs/ops/local-ci-with-agent-priv.md` with:
  - when to use local CI vs GHA
  - commands: agent-priv doctor/request, start-dockerd helper, `swal-ci.sh run <repo>`
  - NoNewPrivs + systemd --user explanation
  - allowlist caution
  - do not paste secrets/tokens

## Web Research Required
1. search: `github actions minutes exhausted local ci alternatives 2025`
2. Reference Ola 10 closeout notes (read-only)

## Exact Technical Context
- CRITICAL: ONLY `docs/ops/local-ci-with-agent-priv.md`
- Do not vendor Hermes skills into the repo

## Problem
Orchestrators and agents lack a canonical in-repo runbook for local CI under sandbox limits.

## Acceptance Criteria
- [ ] Doc exists with agent-priv + swal-ci commands
- [ ] Mentions NoNewPrivs / systemd --user
- [ ] Only listed path added

## Files to Modify
| File | Change | Risk |
|------|--------|------|
| `docs/ops/local-ci-with-agent-priv.md` | NEW | LOW |

## DO NOT touch
- `src/**`, `.github/workflows/**`, features.json

## Verification
```bash
test -f docs/ops/local-ci-with-agent-priv.md
```

## Dependencies & Merge Order
- **Expected effort:** Small
