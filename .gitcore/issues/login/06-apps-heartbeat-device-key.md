# DL-06 — Apps heartbeat + device_key API

| Campo | Valor |
|-------|--------|
| **% validado** | **90%** |
| **Estado** | done (UI residual) |
| **Repos** | maloca `@swal/node`, backoffice, worldexams |

## Scope

`obtainDeviceKeyViaWebAuthn` / PRF + fallback; `startProHeartbeatLoop`; WorldExams mirror.

## Aceptación validada

- [x] API device-key hex 32 B + tests
- [x] Heartbeat loop active/degraded
- [x] Backoffice usa loop compartido
- [ ] Pantalla onboarding Maloca

## Tests (maloca)

`node --experimental-strip-types --test packages/swal-node/src/*.test.ts` → **12 PASS** (sesión 2026-07-28)
