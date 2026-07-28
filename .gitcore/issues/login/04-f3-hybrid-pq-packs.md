# DL-04 — F3 Hybrid PQ pack signatures

| Campo | Valor |
|-------|--------|
| **% validado** | **100%** |
| **Estado** | done |

## Scope

`HybridPackSignature` Ed25519 + ML-DSA commitment; ML-KEM ADR **no-go día-1**.

## Aceptación validada

- [x] Sign/verify Ed25519
- [x] `is_hybrid_ready` con commitment 32 B
- [x] Forged sig rejected
- [x] ADR ML-KEM documentado

## Tests

| Suite | Resultado |
|-------|-----------|
| `node_identity::hybrid_pack` | PASS (2) |
| E2E `e2e_f3_hybrid_pack_sign_verify` | PASS |

## Paths

`src/node_identity/hybrid_pack.rs`, `.gitcore/docs/ADR-SWAL-ML-KEM-DEK.md`
