# DL-05 — F4 Bio / ZKP research (no bloquea)

| Campo | Valor |
|-------|--------|
| **% validado** | **70%** |
| **Estado** | research — spike done 2026-07-29 → **NO-GO hot-path día 1** |

## Scope

ADR research fuzzy extractors / zk-SABER. **Nunca** templates biométricos en Xavier/mesh.

## Done 2026-07-29

- [x] Spike fuzzy extractor local — `docs/SWAL/spikes/fuzzy-extractor/` (fuzzy commitment rep-r/Hamming(7,4); 8 tests verdes; bench N=1000/config, TAR/FAR medidos; helper solo local)
- [x] Eval zk-SABER vs necesidad SWAL — no-PQ (Groth16/BN254), on-chain + Administrator, prototipo sin auditar → no-go/watch-list
- [x] ADR go/no-go TAR/FAR — **NO-GO hot-path día 1** (TAR≥99% @5–10% ruido ⇒ clave 28–36 bits, fuerza-brutable; WebAuthn PRF + vault ya cubren)

## Residual (por qué no 100%)

- Watch-list: re-evaluar solo si threat model escala (robo físico sistemático) **y** ECC k≥128 con TAR≥99% @≥10% ruido con biometría real.
- Producción (BCH/LDPC, pipeline de sensores, PAD anti-spoofing) queda explícitamente no-go'd.

## Doc

`.gitcore/docs/ADR-SWAL-BIO-ZKP-RESEARCH.md` · edge-mesh `F-023`
