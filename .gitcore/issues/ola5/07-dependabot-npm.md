# [Ola 5 · 07] Dependabot: panel-ui npm audit fix (high/moderate)

> Part of #478

## Exact Technical Context
- `panel-ui/package.json` + pnpm
- Run `pnpm audit` / `pnpm update` carefully
- Verify `cd panel-ui && npx vite build`

## Acceptance Criteria
- [ ] Reduce open npm advisories (document before/after counts)
- [ ] vite build passes
- [ ] No intentional secret commits
- [ ] Only panel-ui lockfiles + package.json (+ audit note in PR)

## Merge order
Parallel; do not touch Cargo.lock in this PR.
