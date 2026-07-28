# ADR-SWAL-ML-KEM-DEK — Evaluación ML-KEM para encapsulado DEK

| Campo | Valor |
|-------|--------|
| **ID** | ADR-SWAL-ML-KEM-DEK |
| **Estado** | Aceptado (evaluación Fase 3) |
| **Fecha** | 2026-07-28 |
| **Relacionados** | [DECENTRALIZED_LOGIN.md](./DECENTRALIZED_LOGIN.md) DL-F3-02 · [ADR-SWAL-MESH-GOV.md](./ADR-SWAL-MESH-GOV.md) |

## Decisión (go/no-go)

**No-go para hot path día-1.** ML-KEM (Kyber) queda como **opción evaluada** para envolver DEKs de sealed packs en una fase posterior.

### Motivos

1. Shamir 2-of-3 del DEK / entropy ya cubre recovery offline (Fase 0).
2. Firmas híbridas Ed25519 + ML-DSA commitment (DL-F3-01) priorizan autenticidad del pack sobre encapsulado.
3. Integrar ML-KEM añade superficie (crates, tamaños de ciphertext, UX de re-key) sin desbloquear Pro.

### Cuándo reconsiderar (go)

- Threat model exige forward secrecy de DEK ante compromiso futuro de Ed25519.
- Producto enterprise pide HPKE/ML-KEM explícito en SRC.

### Crates Rust evaluados (referencia)

- `ml-kem` / `pqcrypto-kyber` — disponibles; **no** cableados en Xavier default build.

### Implementación mínima shippable (Fase 3)

- Documentar esta decisión.
- Hybrid pack signatures Ed25519 + commitment ML-DSA en `xavier::node_identity::hybrid_pack`.
- PQ verify e2e en edge-mesh `xavier-bridge` (ya).
