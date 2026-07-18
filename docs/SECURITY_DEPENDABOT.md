# Dependabot / dependency security hygiene

Last verified: **2026-07-18** (Ola 6 closeout). Parent tracker: [#478](https://github.com/iberi22/xavier/issues/478).

## Process

1. Inventory open alerts via GitHub Dependabot API.
2. Prefer **targeted** bumps (`pnpm overrides`, `cargo update -p`, Cargo.toml pin) over blind full upgrades.
3. After every bump: `cargo check --workspace` and (if UI) `pnpm --filter panel-ui build`.
4. Document **deferred** advisories with reason (transitive dual-version, major API break, no patched version in dependency tree).

## npm

| Package | Severity | Action | Status |
|---|---|---|---|
| undici | high | `pnpm.overrides.undici: ^7.28.0` in `panel-ui/package.json` **and** root `package.json` | Done Ola 5/6 |
| esbuild | low | override `^0.28.1` | Done |
| astro | high/medium | pin `astro: ^6.4.6` in panel-ui | Done #651 |

Refresh root lock after override:

```powershell
pnpm install
```

## Cargo (workspace `Cargo.lock`)

| Crate | Severity | Fixed version | Action | Status |
|---|---|---|---|---|
| serde_with | medium | 3.21.0 | Already satisfied in lock | Done |
| jsonwebtoken | medium | 10.3.0 | Direct dep at 9.3 — major API surface; deferred to dedicated PR | Deferred |
| protobuf | medium | 3.7.2 | Transitive 2.28 from legacy parent; no single-package bump | Deferred |
| opentelemetry_sdk | medium | 0.32.1 | Workspace still on 0.18 tree; major | Deferred |
| yamux | high | 0.13.10 | Dual versions 0.12 + 0.13; 0.12 pinned by older libp2p path | Deferred / dual |
| rustls-webpki | high | 0.103.13 | Dual versions 0.101 + 0.103 | Deferred / dual |
| hickory-proto | high | (none listed) | Dual 0.25 + 0.26 | Deferred / dual |
| glib | medium | — | GTK/system; not on default server path | Accepted risk |

### Acceptance for “hygiene complete”

- Inventory documented (this file).
- npm high class remediable via overrides applied.
- Cargo remediable without major parent upgrades applied.
- Remaining deferred items have **owner issue** (#478) and rationale.

This is **process-complete**, not “zero Dependabot alerts forever”.

## Verification

```bash
cargo check --workspace
# optional: gh api repos/iberi22/xavier/dependabot/alerts?state=open
```
