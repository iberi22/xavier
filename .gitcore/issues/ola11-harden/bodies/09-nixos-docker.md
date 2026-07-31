> Parent EPIC: {{EPIC}}

# [Ola 11 · 09] Docs: NixOS Docker without docker.service (dockerd helper)

> Wave **Ola 11 — Harden Residuals**. Ops truth (docs only).

## Current State (MEDIBLE)
- NixOS hosts often lack `docker.service`; Ola 10 used `dockerd` helper via agent-privilege-notify skill (outside repo)
- No in-repo ops doc explaining the constraint

## Desired State (DELTA)
- NEW `docs/ops/nixos-docker.md` documenting:
  - why `systemctl start docker` fails on NixOS without virtualisation.docker
  - recommended paths: enable `virtualisation.docker` OR run dockerd helper + socket group
  - pointer to skill `~/.hermes/skills/devops/agent-privilege-notify` (do not vendor the skill)
  - `/run/wrappers/bin/sudo` note

## Web Research Required
1. search: `NixOS virtualisation.docker service 2025`
2. search: `dockerd rootless nixos 2025`

## Exact Technical Context
- CRITICAL: ONLY create `docs/ops/nixos-docker.md` (create `docs/ops/` if missing)
- NO Rust, NO skill file copies into repo

## Problem
Agents keep trying `systemctl start docker` and fail without documented NixOS reality.

## Acceptance Criteria
- [ ] File exists and mentions `virtualisation.docker` and dockerd helper / agent-priv
- [ ] No secrets
- [ ] Only listed path added

## Files to Modify
| File | Change | Risk |
|------|--------|------|
| `docs/ops/nixos-docker.md` | NEW | LOW |

## DO NOT touch
- `src/**`, `installer/**`, features.json

## Verification
```bash
test -f docs/ops/nixos-docker.md && wc -l docs/ops/nixos-docker.md
```

## Dependencies & Merge Order
- **Expected effort:** Small
