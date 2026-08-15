# FEATURE: feat-ivn-api — Identity Verification HTTP API + Storage

**Status:** `planned` | **Score:** 0% | **Issue:** xavier#1382
**Design:** `docs/SWAL/IDENTITY_VERIFICATION_NETWORK.md` §2 (IVN-3)

## Overview

API REST de verificación de identidad: solicitudes, votos de validadores y consulta de nodos
verificados. El historial de veredictos se almacena en Xavier (SQLite existente) — es la
memoria de la red de identidad.

## Alcance (IN SCOPE)

| ID | Entregable | DoD (verificable por comando) |
|----|-----------|-------------------------------|
| IVN-3-01 | `src/adapters/inbound/http/handlers/ivn.rs` — handler `POST /v1/identity/request` (crear solicitud, applicant + proof hash + firma ML-DSA-65) | `grep -c "v1/identity" src/adapters/inbound/http/routes.rs` >= 5 |
| IVN-3-02 | `GET /v1/identity/request/{id}` — estado pending/approved/rejected | `grep -c "pub async fn" src/adapters/inbound/http/handlers/ivn.rs` >= 5 |
| IVN-3-03 | `POST /v1/identity/{id}/vote` — solo validadores elegidos (403 si no) | test de auth en `tests/ivn_api.rs` |
| IVN-3-04 | `GET /v1/identity/requests` — listado paginado | `wc -l src/adapters/inbound/http/handlers/ivn.rs` >= 120 |
| IVN-3-05 | `GET /v1/identity/verified` — nodos verificados | idem |
| IVN-3-06 | State machine: pending → collecting votes → verdict | test de transiciones en ivn_api.rs |
| IVN-3-07 | Storage en Xavier (reusar store SQLite existente — memory entries con request/vote/verdict) | `cargo test --test ivn_api 2>&1 | grep "test result: ok"` |

## Fuera de alcance (OUT OF SCOPE)

- Lógica de selección/verdict/sanciones → IVN-1 (xavier#1380) — llamar su API, no reimplementar
- Pruebas cifradas → IVN-2 (edge-mesh#95)
- Karma real → IVN-4 (xavier#1381)
- UI → IVN-6 (maloca#80)

## Condiciones de ENTREGA (DoD — TODAS obligatorias)

1. [ ] `cargo build --release --features ci-safe` — 0 errors
2. [ ] `cargo clippy --all-targets -- -D warnings` — 0 warnings
3. [ ] `cargo test --test ivn_api` — test result: ok
4. [ ] `grep -c "v1/identity" src/adapters/inbound/http/routes.rs` >= 5
5. [ ] `wc -l src/adapters/inbound/http/handlers/ivn.rs` >= 120
6. [ ] `git show HEAD --name-only | grep -cE "src/|tests/"` >= 1
7. [ ] PR contiene >= 1 archivo (`gh pr view --json files --jq '.files | length'` >= 1)
8. [ ] NO nuevas dependencias salvo justificación explícita
9. [ ] Golden rule: no Rayon en Tokio — spawn_blocking

## Verification harness

```bash
cd /home/belal/proyectosSWAL/apps/xavier
nix-shell -p openssl.dev pkg-config --run "cargo build --release --features ci-safe"
nix-shell -p openssl.dev pkg-config --run "cargo clippy --all-targets -- -D warnings"
nix-shell -p openssl.dev pkg-config --run "cargo test --test ivn_api"
```

## Anti-hallucination

- READ before write: `ivn.rs` (tras IVN-1) + `handlers/marketplace.rs` (el patrón MÁS NUEVO de Wave M #1379)
- Seguir el patrón del handler marketplace.rs (NO el viejo memory.rs)
- Vote auth: 403 si el votante no está en los validadores elegidos
- Storage: reuse store existente — no inventar capa DB nueva
- No tocar data_commons/ (solo llamar su API) · features.json reconciliado al final
