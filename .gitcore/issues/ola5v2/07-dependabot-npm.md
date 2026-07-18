# [Ola 5v2 · 07] Dependabot: remediate panel-ui npm high/moderate alerts

> **Re-launch** split of #478 (npm track). Tracker parent remains #478.

## Web Research Required (Jules must search the web)

1. **pnpm audit workflow** — search: `pnpm audit fix 2024 2025 documentation`, read https://pnpm.io/cli/audit
2. **Vite security advisories** — search: `vite CVE security advisory 2024 2025 npm`, ensure upgrade path compatible with Tauri panel.
3. **Transitive undici / fetch cookie issues** — search: `undici Set-Cookie SameSite vulnerability npm 2025 advisory`.

Also pull live repo alerts via GitHub Dependabot UI or API for ecosystem=npm.

PR must include before/after audit summary.

## Exact Technical Context

- Directory: `panel-ui/`
- Package manager: **pnpm** (repo convention — NOT npm install)
- Build gate: `cd panel-ui && npx vite build` (or `pnpm exec vite build`)
- Do not bump majors that break Tauri without documenting why

> CRITICAL: Only `panel-ui/package.json` + lockfile. DO NOT touch Cargo.lock. DO NOT commit secrets. NEVER `.patch` files.

## Problem

Large share of Dependabot findings are npm/panel-ui transitive deps; main stays noisy and risky.

## Acceptance Criteria

- [ ] Record before count from `pnpm audit` (or gh dependabot npm filter)
- [ ] Apply safe fixes; record after count
- [ ] `vite build` succeeds
- [ ] List remaining unfixed with rationale
- [ ] Empty PR forbidden

## Files to Modify

| File | Change |
|---|---|
| `panel-ui/package.json` | version bumps |
| `panel-ui/pnpm-lock.yaml` | lock |

**DO NOT touch:** `src/**/*.rs`, `Cargo.toml`, `Cargo.lock`

## Verification

```bash
cd panel-ui && pnpm audit || true
cd panel-ui && npx vite build
```

## Dependencies and Merge Order

- **Depends on:** nothing
- **Must merge before 08** (serialize dependency churn): **07 then 08**
