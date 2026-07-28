# ADR-SWAL-BIO-ZKP-RESEARCH — Fuzzy / ZKP biométrico (Fase 4)

| Campo | Valor |
|-------|--------|
| **ID** | ADR-SWAL-BIO-ZKP-RESEARCH |
| **Estado** | Research tracked — **siguiente fase de roadmap login** (fuera del DoD shippable) |
| **Fecha** | 2026-07-28 |
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
| DL-F4-01 | Spike fuzzy extractor (helper local; sin template en red) | pending |
| DL-F4-02 | Lectura crítica zk-SABER vs necesidad SWAL | pending |
| DL-F4-03 | ADR go/no-go con threat model + TAR/FAR | pending (este doc es el contenedor) |

### Orden sugerido del spike

1. Inventario de sensores / WebAuthn-only vs biometría “raw” (preferir WebAuthn PRF antes que fuzzy).
2. Spike offline: fuzzy extractor → clave + helper data; tests de reconstrucción con ruido simulado.
3. Comparar costo/UX vs BIP39+Shamir+PIN ya shippable.
4. Solo si hay amenaza concreta (robo físico sistemático) → evaluar ZKP (zk-SABER paper); si no, **no-go**.

## Relación con feature 100%

`feat-decentralized-login` **done** = Fases 0–3 shippable + este ADR marcando F4 como research separado.

## Lectura

- [DECENTRALIZED_LOGIN_PROGRESS.md](./DECENTRALIZED_LOGIN_PROGRESS.md) §3.C
- Paper zk-SABER (verificar claim vs implementación real antes de adoptar)
