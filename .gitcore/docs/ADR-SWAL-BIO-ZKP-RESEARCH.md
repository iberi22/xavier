# ADR-SWAL-BIO-ZKP-RESEARCH — Fuzzy / ZKP biométrico (Fase 4)

| Campo | Valor |
|-------|--------|
| **ID** | ADR-SWAL-BIO-ZKP-RESEARCH |
| **Estado** | Research tracked — **siguiente fase de roadmap login** (fuera del DoD shippable) · spike ejecutado 2026-07-29 → **NO-GO hot-path día 1** (ver §Veredicto go/no-go) |
| **Fecha** | 2026-07-28 (spike: 2026-07-29) |
| **Relacionados** | [DECENTRALIZED_LOGIN.md](./DECENTRALIZED_LOGIN.md) §Fase 4 · [DECENTRALIZED_LOGIN_PROGRESS.md](./DECENTRALIZED_LOGIN_PROGRESS.md) · LOGIN_IDENTITY_DESIGN §6 · edge-mesh `F-023` |

## Veredicto

**No bloquea Pro ni el 100% shippable de `feat-decentralized-login` (F0–F3).**  
Fase 4 es el **siguiente track de roadmap de login** después del cierre shippable.

Paralelo recomendado para producción: **ops deploy** del registry Polygon en Amoy (ver PROGRESS §3.A) — no es research F4.

## Reglas inviolables

- Helper data **local**; **nunca** templates biométricos en Xavier memory ni mesh.
- No hot-path de login con ZKP biométrico hasta go/no-go con TAR/FAR medidos.

## Trabajo futuro (checklist Fase 4)

| ID | Entregable | Estado |
|----|------------|--------|
| DL-F4-01 | Spike fuzzy extractor (helper local; sin template en red) | **done 2026-07-29** — `docs/SWAL/spikes/fuzzy-extractor/` (fuzzy commitment rep-r/Hamming(7,4), 8 tests verdes, bench N=1000) |
| DL-F4-02 | Lectura crítica zk-SABER vs necesidad SWAL | **done 2026-07-29** — ver §Evaluación zk-SABER |
| DL-F4-03 | ADR go/no-go con threat model + TAR/FAR | **done 2026-07-29** — ver §Veredicto go/no-go (este doc) |

### Orden sugerido del spike

1. Inventario de sensores / WebAuthn-only vs biometría “raw” (preferir WebAuthn PRF antes que fuzzy).
2. Spike offline: fuzzy extractor → clave + helper data; tests de reconstrucción con ruido simulado.
3. Comparar costo/UX vs BIP39+Shamir+PIN ya shippable.
4. Solo si hay amenaza concreta (robo físico sistemático) → evaluar ZKP (zk-SABER paper); si no, **no-go**.

## Resultados del spike (2026-07-29) — DL-F4-01

Spike: `docs/SWAL/spikes/fuzzy-extractor/` (Node `.mjs`, sin deps; mismo tooling que `spikes/sealed-pack/`).
Esquema: fuzzy commitment (Juels–Wattenberg) — `enroll(W) -> (K, helper={mask=C(K)⊕W, check=SHA-256(K)})`, `reproduce(W',helper) -> K | fallo`.
Biometría SIMULADA: vectores uniformes de 256 bits; ruido flip-bit; N=1000 iteraciones/config; seed fija (reproducible).

Tabla medida (resumen; tabla completa en el README del spike):

| Esquema | Clave (bits) | TAR @1% | TAR @5% | TAR @10% | TAR @15% | FAR (todas las tasas) |
|---------|--------------|---------|---------|----------|----------|------------------------|
| rep-1 (sin ECC) | 256 | 9.0% | 0.0% | 0.0% | 0.0% | 0/1000 (<0.1%) |
| hamming-7-4 | 144 | 92.7% | 19.3% | 0.1% | 0.0% | 0/1000 (<0.1%) |
| rep-3 | 85 | 97.5% | 55.3% | 9.4% | 0.4% | 0/1000 (<0.1%) |
| rep-5 | 51 | 99.9% | 94.3% | 61.1% | 26.6% | 0/1000 (<0.1%) |
| rep-7 | 36 | 100.0% | 99.1% | 90.3% | 64.4% | 0/1000 (<0.1%) |
| rep-9 | 28 | 100.0% | 99.9% | 97.5% | 84.0% | 0/1000 (<0.1%) |

FAR 0/1000 ⇒ cota superior 95% ≈ 0.30% (regla del tres); FAR online estructural ~2^-k por intento.

**Hallazgo decisivo:** con ECC simple, TAR≥99% a ruido 5% exige claves de 28–36 bits. Como el helper es **local**, la seguridad en reposo ante robo del dispositivo ES la entropía de la clave: 2^28–2^36 es fuerza-brutable trivialmente. Llegar a ≥128 bits con ruido 5–15% requiere BCH/LDPC real + biometría de mayor entropía efectiva (proyecto, no spike) — y la biometría real (no uniforme, bits correlacionados) empeora los números.

## Threat model (DL-F4-03)

Amenazas consideradas vs cobertura actual (WebAuthn PRF + vault BIP39+Shamir+PIN + mesh challenge Ed25519/ML-DSA):

| Amenaza | ¿Cubierta hoy? | ¿Fuzzy/ZKP ayudaría? |
|---------|----------------|----------------------|
| Phishing / robo remoto de credenciales | Sí (WebAuthn PRF, challenge firmado) | No aporta |
| Pérdida del dispositivo (recovery) | Sí (Shamir 2-of-3, DL-01) | No aporta |
| Compromiso de la mesh / Xavier memory | Sí (no hay secrets biométricos ahí — regla inviolable) | No aplica |
| **Robo físico sistemático del dispositivo con extracción de secrets en reposo** | Parcial (vault cifrado 256-bit + PIN rate-limited resiste) | **Única amenaza que lo justificaría** — pero con k=28–51 bits medidos, el helper biométrico resistiría *peor* que el vault actual |

## Evaluación zk-SABER (DL-F4-02)

Paper: *zkSABER: Zero-knowledge Succinct Authentication using Biometric Embedding Representation* (Chinen et al., IEEE BRAINS 2025). Lectura crítica completa en el README del spike; resumen:

1. **No es lattice-based**: es Groth16 sobre BN254 (pairings) → **no post-cuántico**; desalineado con la identidad PQ de SWAL (ML-DSA-65, F-022).
2. **On-chain + Administrator honesto** (Merkle tree de usuarios registrados): rol centralizador; contradice "mesh is NOT the ledger" y el login local-first/offline.
3. **Trusted setup** por circuito (Groth16).
4. **Implementación prototipo** (ZoKrates, código de conferencia; proving time reconocido como cuello de botella). Sin auditoría ni PAD (anti-spoofing).
5. Asume embeddings DNN de sensores reales — fuera del alcance de SWAL (spike con biometría simulada; sin roadmap de sensores).

**Veredicto zk-SABER:** resuelve verificación biométrica anónima on-chain, un problema que SWAL no tiene en el hot-path. **No-go; watch-list** (re-evaluar solo si se dan a la vez: robo físico sistemático + necesidad de prueba biométrica anónima verificable por terceros).

## Veredicto go/no-go (2026-07-29)

**NO-GO para hot-path de login día 1.** Argumentado por las mediciones:

1. **Seguridad en reposo insuficiente**: para TAR≥99% a ruido realista (5–10%) el ECC simple solo sostiene claves de 28–36 bits (tabla medida) → helper local fuerza-brutable. Inaceptable como único factor y peor que el vault ya shippable.
2. **Sin ventaja sobre lo existente**: WebAuthn PRF (`@swal/node`) + vault BIP39+Shamir+PIN ya cubren la UX biométrica hardware-backed sin templates reversibles ni superficie PAD.
3. **Coste desproporcionado**: un fuzzy extractor serio exige ECC real (BCH/LDPC), pipeline de sensores, PAD y medición de entropía real por modalidad — un proyecto completo para una amenaza (robo físico sistemático) no priorizada hoy.
4. **ZKP biométrico (zk-SABER) no aplica**: no-PQ, on-chain, prototipo sin auditar (ver §Evaluación zk-SABER).

**Condiciones de re-apertura (go):** amenaza concreta y priorizada de robo físico sistemático de dispositivos **y** un ECC con k≥128 bits que mantenga TAR≥99% a ruido ≥10% medido con biometría real. Hasta entonces: watch-list, sin código en hot-path. Regla inviolable mantenida: helper local, nunca templates en Xavier memory ni mesh.

## Relación con feature 100%

`feat-decentralized-login` **done** = Fases 0–3 shippable + este ADR marcando F4 como research separado.

## Lectura

- [DECENTRALIZED_LOGIN_PROGRESS.md](./DECENTRALIZED_LOGIN_PROGRESS.md) §3.C
- Paper zk-SABER (verificar claim vs implementación real antes de adoptar)
