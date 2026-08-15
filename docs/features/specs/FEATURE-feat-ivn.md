# FEATURE: feat-ivn — Identity Verification Core (Verifict + Selection + Sanctions)

**Status:** `planned` | **Score:** 0% (diseño aprobado 2026-08-15) | **Issue:** xavier#1380
**Design:** `docs/SWAL/IDENTITY_VERIFICATION_NETWORK.md` §2 (IVN-1)

## Overview

Motor de verificación de identidad de la red: selección karma-ponderada de validadores,
evaluación de votos con quórum dinámico y sanciones. Es la raíz de la pirámide de la verdad
de Xavier — sin identidad real verificada, la afinación humana (HumanChallenge) y la
reputación (EigenTrust) son manipulables por bots.

## Alcance (IN SCOPE)

| ID | Entregable | DoD (verificable por comando) |
|----|-----------|-------------------------------|
| IVN-1-01 | `src/data_commons/ivn.rs` — IvnConfig (validators_per_request=5, karma_min_validator=300, quorum_ratio=0.8, karma_pow=2.0, retry_days=30, penalty_false_positive=-10, penalty_lie=-50, exclusion_days=90) | `grep -c "validators_per_request\|karma_min_validator" src/data_commons/ivn.rs` >= 2 |
| IVN-1-02 | `ValidatorSelection::select_validators(node_pool, exclude_seed, rng)` — karma^2 ponderado, excluye nodos que comparten seed (anti-sybil, self-dealing de EigenTrust) | `grep -c "fn select_validators" src/data_commons/ivn.rs` >= 1 + test de ponderación |
| IVN-1-03 | `VerdictEngine::evaluate_votes(votes, quorum) -> Verdict` — Vote{Check,Reject,Abstain}, quórum dinámico via `effective_user_quorum` (governance.rs DynamicQuorum) | `grep -c "fn evaluate_votes" src/data_commons/ivn.rs` >= 1 + test quórum 4/5 |
| IVN-1-04 | `sanction_validator(fp_count)` — penalización + exclusión (stub para IVN-4) | `grep -c "fn sanction_validator" src/data_commons/ivn.rs` >= 1 |
| IVN-1-05 | Wire VerdictEngine → DynamicQuorum en governance.rs (reuso, mínimo) | `grep -c "effective_user_quorum" src/data_commons/ivn.rs` >= 1 |
| IVN-1-06 | Export `ivn` en `src/data_commons/mod.rs` | `grep -c "pub mod ivn" src/data_commons/mod.rs` >= 1 |
| IVN-1-07 | `tests/ivn_verdict.rs` — selección ponderada, exclusión seed, quórum, abstención | `cargo test --test ivn_verdict 2>&1 | grep "test result: ok"` — 1 match |

## Fuera de alcance (OUT OF SCOPE)

- Pruebas E2E cifradas → IVN-2 (edge-mesh#95)
- API HTTP `/v1/identity/*` → IVN-3 (xavier#1382)
- Aplicación real de karma/sanciones → IVN-4 (xavier#1381, SECUENCIAL tras este)
- Merkle on-chain + X2.1 → IVN-5 (gara-g#709)
- UI → IVN-6 (maloca#80)

## Condiciones de ENTREGA (Definition of Done — TODAS obligatorias)

1. [ ] `cargo build --release --features ci-safe` — 0 errors
2. [ ] `cargo clippy --all-targets -- -D warnings` — 0 warnings
3. [ ] `cargo test --test ivn_verdict` — test result: ok
4. [ ] `wc -l src/data_commons/ivn.rs` >= 120 (implementación real, no stub)
5. [ ] `git show HEAD --name-only | grep -cE "src/|tests/"` >= 1 (archivos fuente reales)
6. [ ] PR contiene >= 1 archivo (`gh pr view --json files --jq '.files | length'` >= 1)
7. [ ] `git diff --stat HEAD` lista archivos (NO vacío)
8. [ ] NO nuevas dependencias salvo justificación explícita en el PR
9. [ ] Golden rule: no Rayon `.par_iter()` dentro de Tokio workers — `spawn_blocking`

## Verification harness

```bash
cd /home/belal/proyectosSWAL/apps/xavier
nix-shell -p openssl.dev pkg-config --run "cargo build --release --features ci-safe"
nix-shell -p openssl.dev pkg-config --run "cargo clippy --all-targets -- -D warnings"
nix-shell -p openssl.dev pkg-config --run "cargo test --test ivn_verdict"
```

## Anti-hallucination

- READ before write: `governance.rs` + `reputation.rs` FULLY antes de codificar
- Reusar `rand::distributions::WeightedIndex` (no implementar weighted random a mano)
- KISS: lógica pura, sin DB, sin red
- No tocar `reputation.rs` / `pricing.rs` / `marketplace.rs` / `mesh_bridge.rs`
- `docs/features/features.json` reconciliado al final de la wave (nunca a mano)
