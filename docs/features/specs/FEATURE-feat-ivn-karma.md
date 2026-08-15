# FEATURE: feat-ivn-karma — Karma Rewards & Sanctions

**Status:** `planned` | **Score:** 0% | **Issue:** xavier#1381
**Design:** `docs/SWAL/IDENTITY_VERIFICATION_NETWORK.md` §2.2/§2.3 (IVN-4) + ivn_sim.py (valores simulados validados)

## Overview

Aplicación de recompensas y sanciones de karma del IVN. El karma es híbrido (soulbound +
EigenTrust, decisión b de ECON_CORE_MESH). Sanciones integradas con EigenTrustEngine —
no bypass. El karma verificado funciona como aval social (más tareas, mejor pricing,
más peso en challenges) y su pérdida es la sanción natural (pierde beneficios de
selección para validación).

## Parámetros EXACTOS (del diseño — NO inventar)

| Param | Valor | Constante en ivn.rs |
|-------|-------|---------------------|
| Karma nodo verificado | **+20** | `bonus_karma_verified` |
| Karma validador correcto | **+5** | `bonus_karma_validator_ok` |
| Karma abstención | **+1** | `bonus_karma_abstain` |
| Sanción falso positivo | **-10** | `penalty_karma_false_positive` |
| Sanción solicitante miente | **-50** | `penalty_karma_lie` |
| Exclusión validador falso | **90 días** | `exclusion_days` |
| Espera solicitante miente | **180 días** | `retry_days` (solicitante) |

## Alcance (IN SCOPE)

| ID | Entregable | DoD (verificable por comando) |
|----|-----------|-------------------------------|
| IVN-4-01 | `apply_rewards(verdict, participants)` — +20 verified, +5 correctos, +1 abstenciones | `grep -c "fn apply_rewards\|fn apply_sanctions\|fn is_excluded" src/data_commons/ivn.rs` >= 3 |
| IVN-4-02 | `apply_sanctions(fp_validators, liar)` — -10 + 90d / -50 + 180d, vía EigenTrustEngine | idem |
| IVN-4-03 | `is_excluded(node_id)` — consulta de exclusión (usada por IVN-1 selection) | idem |
| IVN-4-04 | `adjust_karma(node, delta)` en reputation.rs (si no existe — mínima) | `grep -c "adjust_karma" src/data_commons/reputation.rs src/data_commons/ivn.rs` >= 1 |
| IVN-4-05 | Constantes de recompensa/sanción en IvnConfig | `grep -c "bonus_karma_verified\|penalty_karma_false_positive\|penalty_karma_lie" src/data_commons/ivn.rs` >= 3 |
| IVN-4-06 | `tests/ivn_karma.rs` — deltas exactos (+20/+5/+1/-10/-50), exclusión, consistencia con selection | `cargo test --test ivn_karma 2>&1 | grep "test result: ok"` |

## Fuera de alcance (OUT OF SCOPE)

- Selección/verdict → IVN-1 (xavier#1380) — este issue solo EXTIEENDE ivn.rs
- API → IVN-3 (xavier#1382)
- Merkle on-chain → IVN-5 (gara-g#709)
- UI → IVN-6 (maloca#80)

## Condiciones de ENTREGA (DoD — TODAS obligatorias)

1. [ ] `cargo build --release --features ci-safe` — 0 errors
2. [ ] `cargo clippy --all-targets -- -D warnings` — 0 warnings
3. [ ] `cargo test --test ivn_karma` — test result: ok
4. [ ] Deltas EXACTOS +20/+5/+1/-10/-50 (test lo verifica)
5. [ ] Sanciones pasan por EigenTrustEngine (NO store separado)
6. [ ] `is_excluded` consistente con selection de IVN-1 (90d/180d)
7. [ ] `git show HEAD --name-only | grep -cE "src/|tests/"` >= 1
8. [ ] PR contiene >= 1 archivo (`.files | length` >= 1)
9. [ ] Golden rule: no Rayon en Tokio — spawn_blocking

## Verification harness

```bash
cd /home/belal/proyectosSWAL/apps/xavier
nix-shell -p openssl.dev pkg-config --run "cargo build --release --features ci-safe"
nix-shell -p openssl.dev pkg-config --run "cargo clippy --all-targets -- -D warnings"
nix-shell -p openssl.dev pkg-config --run "cargo test --test ivn_karma"
```

## Anti-hallucination

- READ before write: `reputation.rs` FULLY (cómo funciona karma híbrido + EigenTrust)
- SECUENCIAL tras IVN-1: si ivn.rs no existe aún, implementar contra el diseño §2.2 + TODO
- Valores exactos del spec — NO rediseñar karma (decisión b ya tomada)
- No tocar governance/marketplace/pricing/mesh_bridge · features.json al final
